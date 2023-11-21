use ed25519::Signature;
use ed25519_dalek::{Signer, SigningKey, Verifier};
use hex;
use hyper::server::accept::from_stream;
use hyper::Server;
use sha2::{Digest, Sha512};
use std::ffi::OsString;
use std::net::{IpAddr, Ipv4Addr};
use std::os::unix::ffi::OsStringExt;
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

const RMC_THRESHOLD: u64 = 3;

fn main() {
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "info");
    }

    tracing_subscriber::fmt::init();

    let addr0 = (IpAddr::from(Ipv4Addr::UNSPECIFIED), 9999);
    let addr1 = (IpAddr::from(Ipv4Addr::UNSPECIFIED), 9998);
    let addr2 = (IpAddr::from(Ipv4Addr::UNSPECIFIED), 9997);
    let (tx, _rx) = broadcast::channel::<RmcPeerMessage>(16);
    let tx0 = tx.clone();
    let tx1 = tx.clone();
    let tx2 = tx.clone();
    let (signing_key_0, greeter0) = generate_multicast_server(tx0.clone(), &[0u8; 32]);
    let (signing_key_1, greeter1) = generate_multicast_server(tx1.clone(), &[1u8; 32]);
    let (signing_key_2, greeter2) = generate_multicast_server(tx2.clone(), &[2u8; 32]);

    let mut sim = Builder::new().build();

    sim.host("server0", move || {
        let greeter = greeter0.clone();
        let signing_key = signing_key_0.clone();
        let tx = tx0.clone(); // Clone tx here
        async move {
            Server::builder(from_stream(async_stream::stream! {
                let listener = net::TcpListener::bind(addr0).await?;
                let mut rx = tx.clone().subscribe();
                loop {
                    tokio::select! {
                        res = listener.accept() => {
                            yield res.map(|(s, _)| s);
                        }
                        peer_message = rx.recv() => {
                            let mut peer_message = peer_message.unwrap();
                            let digest = peer_message.data.clone();
                            let mut signatures = peer_message.signatures;

                            let my_sig = signing_key.sign(&digest);
                            signatures.push(my_sig);
                            peer_message.signatures = signatures.clone();

                            if (signatures.len() as u64) < RMC_THRESHOLD {
                                match tx.send(peer_message) {
                                    Ok(_) => {},
                                    Err(_) => println!("Failed to send message to peers"),
                                }
                            }

                            let comma_separated: String = signatures.iter()
                                .map(|sig| sig.to_vec())
                                .map(|bytes| bytes.iter().map(|&x| x.to_string()).collect::<Vec<String>>().join(","))
                                .collect::<Vec<String>>()
                                .join(",");
                            let path = OsString::from_vec(hex::encode(&digest.clone()).into());
                            if !turmoil::has(path.clone()).await {
                                let _ = turmoil::append(path, comma_separated.into()).await;
                            } else {
                                let _ = turmoil::write(path, comma_separated.into()).await;
                            }
                        }
                    }
                }
            }))
            .serve(Shared::new(greeter))
            .await
            .unwrap();

            Ok(())
        }
        .instrument(info_span!("server0"))
    });

    sim.host("server1", move || {
        let greeter = greeter1.clone();
        let signing_key = signing_key_1.clone();
        let tx = tx1.clone(); // Clone tx here
        async move {
            Server::builder(from_stream(async_stream::stream! {
                let listener = net::TcpListener::bind(addr1).await?;
                let mut rx = tx.clone().subscribe();
                loop {
                    tokio::select! {
                        res = listener.accept() => {
                            yield res.map(|(s, _)| s);
                        }
                        peer_message = rx.recv() => {
                            let mut peer_message = peer_message.unwrap();
                            let digest = peer_message.data.clone();
                            let mut signatures = peer_message.signatures;

                            let my_sig = signing_key.sign(&digest);
                            signatures.push(my_sig);
                            peer_message.signatures = signatures.clone();

                            if (signatures.len() as u64) < RMC_THRESHOLD {
                                match tx.send(peer_message) {
                                    Ok(_) => {},
                                    Err(_) => println!("Failed to send message to peers"),
                                }
                            }

                            let comma_separated: String = signatures.iter()
                                .map(|sig| sig.to_vec())
                                .map(|bytes| bytes.iter().map(|&x| x.to_string()).collect::<Vec<String>>().join(","))
                                .collect::<Vec<String>>()
                                .join(",");
                            let path = OsString::from_vec(hex::encode(&digest.clone()).into());
                            let _ = turmoil::write(path, comma_separated.into()).await;
                        }
                    }
                }
            }))
            .serve(Shared::new(greeter))
            .await
            .unwrap();

            Ok(())
        }
        .instrument(info_span!("server1"))
    });

    sim.host("server2", move || {
        let greeter = greeter2.clone();
        let signing_key = signing_key_2.clone();
        let tx = tx2.clone(); // Clone tx here
        async move {
            Server::builder(from_stream(async_stream::stream! {
                let listener = net::TcpListener::bind(addr2).await?;
                let mut rx = tx.clone().subscribe();
                loop {
                    tokio::select! {
                        res = listener.accept() => {
                            yield res.map(|(s, _)| s);
                        }
                        peer_message = rx.recv() => {
                            let mut peer_message = peer_message.unwrap();
                            let digest = peer_message.data.clone();
                            let mut signatures = peer_message.signatures;

                            let my_sig = signing_key.sign(&digest);
                            signatures.push(my_sig);
                            peer_message.signatures = signatures.clone();

                            if (signatures.len() as u64) < RMC_THRESHOLD {
                                match tx.send(peer_message) {
                                    Ok(_) => {},
                                    Err(_) => println!("Failed to send message to peers"),
                                }
                            }

                            let comma_separated: String = signatures.iter()
                                .map(|sig| sig.to_vec())
                                .map(|bytes| bytes.iter().map(|&x| x.to_string()).collect::<Vec<String>>().join(","))
                                .collect::<Vec<String>>()
                                .join(",");
                            let path = OsString::from_vec(hex::encode(&digest.clone()).into());
                            let _ = turmoil::write(path, comma_separated.into()).await;
                        }
                    }
                }
            }))
            .serve(Shared::new(greeter))
            .await
            .unwrap();

            Ok(())
        }
        .instrument(info_span!("server2"))
    });

    sim.client(
        "client0",
        async move {
            let ch = Endpoint::new("http://server0:9999")?
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
        .instrument(info_span!("client0")),
    );

    sim.client(
        "client1",
        async move {
            let ch = Endpoint::new("http://server1:9998")?
                .connect_with_connector(connector::connector())
                .await?;
            let mut greeter_client = MulticastClient::new(ch);

            let request = Request::new(RmcMessage {
                message: "gabba goo".into(),
            });
            let res = greeter_client.send(request).await?;

            tracing::info!("Got response: {:?}", res);

            Ok(())
        }
        .instrument(info_span!("client1")),
    );

    sim.run().unwrap();
}

#[derive(Clone, Debug)]
struct RmcPeerMessage {
    data: Vec<u8>,
    signatures: Vec<Signature>,
}

pub struct MyMulticaster<S>
where
    S: Signer<Signature>,
{
    sender: broadcast::Sender<RmcPeerMessage>,
    pub signing_key: S,
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
            signatures: Vec::from([sig]),
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

fn generate_multicast_server(
    tx: broadcast::Sender<RmcPeerMessage>,
    signing_key: &[u8; 32],
) -> (SigningKey, MulticastServer<MyMulticaster<SigningKey>>) {
    let signing_key = SigningKey::from_bytes(signing_key);
    (
        signing_key.clone(),
        MulticastServer::new(MyMulticaster {
            sender: tx.clone(),
            signing_key,
        }),
    )
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
