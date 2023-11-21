use ed25519::Signature;
use ed25519_dalek::{Signer, SigningKey, Verifier};
use hyper::server::accept::from_stream;
use hyper::Server;
use sha2::{Digest, Sha512};
use std::net::{IpAddr, Ipv4Addr};
use tokio::sync::broadcast;
use tonic::transport::Endpoint;
use tonic::Status;
use tonic::{Request, Response};
use tower::make::Shared;
use tracing::{info_span, Instrument};
use turmoil::{net, Builder};

use std::marker::{Send, Sync};

#[allow(non_snake_case)]
mod proto {
    tonic::include_proto!("rmc");
}

use proto::multicast_server::{Multicast, MulticastServer};
use proto::{RmcMessage, RmcResponse};

use crate::proto::multicast_client::MulticastClient;

fn main() {
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "info");
    }

    tracing_subscriber::fmt::init();

    let addr0 = (IpAddr::from(Ipv4Addr::UNSPECIFIED), 9999);
    // let addr1 = (IpAddr::from(Ipv4Addr::UNSPECIFIED), 9999);

    let mut sim = Builder::new().build();

    let (tx, _rx) = broadcast::channel::<RmcPeerMessage>(1);

    let signing_key: SigningKey = SigningKey::from_bytes(&[0u8; 32]);

    let greeter = MulticastServer::new(MyMulticaster {
        sender: tx.clone(),
        signing_key,
    });

    sim.host("server", move || {
        let greeter = greeter.clone();
        let tx = tx.clone(); // Clone tx here
        async move {
            Server::builder(from_stream(async_stream::stream! {
                let listener = net::TcpListener::bind(addr0).await?;
                let mut rx = tx.clone().subscribe();
                loop {
                    tokio::select! {
                        res = listener.accept() => {
                            yield res.map(|(s, _)| s);
                        }
                        _ = rx.recv() => {
                            // Handle broadcast message
                        }
                    }
                }
            }))
            .serve(Shared::new(greeter))
            .await
            .unwrap();

            Ok(())
        }
        .instrument(info_span!("server"))
    });

    sim.client(
        "client",
        async move {
            let ch = Endpoint::new("http://server:9998")?
                .connect_with_connector(connector::connector())
                .await?;
            let mut greeter_client = MulticastClient::new(ch);

            let request = Request::new(RmcMessage {
                message: "foo".into(),
            });
            let res = greeter_client.send(request).await?;

            tracing::info!("Got response: {:?}", res);

            Ok(())
        }
        .instrument(info_span!("client")),
    );

    sim.run().unwrap();
}

#[derive(Clone, Debug)]
struct RmcPeerMessage {
    data: Vec<u8>,
    signature: Vec<Signature>,
}

pub struct MyMulticaster<S>
where
    S: Signer<Signature>,
{
    sender: broadcast::Sender<RmcPeerMessage>,
    signing_key: S,
}

impl<S> MyMulticaster<S> where S: Signer<Signature> {}

#[tonic::async_trait]
impl<S> Multicast for MyMulticaster<S>
where
    S: 'static + Signer<Signature> + Send + Sync,
{
    async fn send(&self, request: Request<RmcMessage>) -> Result<Response<RmcResponse>, Status> {
        let message = request.into_inner();
        println!("Received message: {}", message.message);

        let mut hasher = Sha512::new();
        hasher.update(message.clone().message);
        let hash = hasher.finalize();
        let sig = self.signing_key.sign(&hash);

        let peer_message = RmcPeerMessage {
            data: hash.to_vec(),
            signature: Vec::from([sig]),
        };

        // Send the message to all peers.
        match self.sender.send(peer_message) {
            Ok(_) => println!("Message sent to peers"),
            Err(_) => println!("Failed to send message to peers"),
        }

        Ok(Response::new(RmcResponse {
            message: "Message received".to_string(),
        }))
    }
}

mod connector {
    use std::{future::Future, pin::Pin};

    use hyper::{
        client::connect::{Connected, Connection},
        Uri,
    };
    use tokio::io::{AsyncRead, AsyncWrite};
    use tower::Service;
    use turmoil::net::TcpStream;

    type Fut = Pin<Box<dyn Future<Output = Result<TurmoilConnection, std::io::Error>> + Send>>;

    pub fn connector(
    ) -> impl Service<Uri, Response = TurmoilConnection, Error = std::io::Error, Future = Fut> + Clone
    {
        tower::service_fn(|uri: Uri| {
            Box::pin(async move {
                let conn = TcpStream::connect(uri.authority().unwrap().as_str()).await?;
                Ok::<_, std::io::Error>(TurmoilConnection(conn))
            }) as Fut
        })
    }

    pub struct TurmoilConnection(turmoil::net::TcpStream);

    impl AsyncRead for TurmoilConnection {
        fn poll_read(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &mut tokio::io::ReadBuf<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            Pin::new(&mut self.0).poll_read(cx, buf)
        }
    }

    impl AsyncWrite for TurmoilConnection {
        fn poll_write(
            mut self: Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &[u8],
        ) -> std::task::Poll<Result<usize, std::io::Error>> {
            Pin::new(&mut self.0).poll_write(cx, buf)
        }

        fn poll_flush(
            mut self: Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<(), std::io::Error>> {
            Pin::new(&mut self.0).poll_flush(cx)
        }

        fn poll_shutdown(
            mut self: Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<(), std::io::Error>> {
            Pin::new(&mut self.0).poll_shutdown(cx)
        }
    }

    impl Connection for TurmoilConnection {
        fn connected(&self) -> hyper::client::connect::Connected {
            Connected::new()
        }
    }
}
