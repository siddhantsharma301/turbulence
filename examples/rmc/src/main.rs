use std::ffi::OsString;
use std::marker::{Send, Sync};
use std::net::{IpAddr, Ipv4Addr};
use std::os::unix::ffi::OsStringExt;

// Turmoil and server imports
use hyper::server::accept::from_stream;
use hyper::Server;
use tokio::sync::broadcast;
use tonic::transport::Endpoint;
use tonic::Status;
use tonic::{Request, Response};
use tower::make::Shared;
use tracing::Instrument;
use turmoil::{net, Builder, Sim, TurmoilMessage};

// Application specific imports
use ed25519::Signature;
use ed25519_dalek::{Signer, SigningKey, Verifier, VerifyingKey};
use hex;
use rand::Rng;
use rand::{rngs::SmallRng, RngCore, SeedableRng};
use sha2::{Digest, Sha512};

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

    let rng = SmallRng::from_entropy();

    let mut sim = generate_server_client_configuration(rng);
    sim.run().unwrap();
}

fn generate_server_client_configuration(mut rng: SmallRng) -> Sim<'static> {
    let mut sim = Builder::new().rng(rng.clone()).build();
    let num_server = rng.gen_range(2..10);
    let num_client = rng.gen_range(1..num_server);

    let (tx, _) = broadcast::channel::<RmcPeerMessage>(16);

    for i in 0..num_server {
        let server_name = format!("server{}", i);
        let port = 9999 - i as u16;
        let addr = (IpAddr::from(Ipv4Addr::UNSPECIFIED), port);
        let tx = tx.clone();
        let (signing_key, greeter) = generate_multicast_server(tx.clone(), &[i; 32]);
        let rng = SmallRng::from_entropy();

        sim.host(server_name, move || {
            let greeter = greeter.clone();
            let signing_key = signing_key.clone();
            let tx = tx.clone();
            let rng = rng.clone();
            async move {
                Server::builder(from_stream(async_stream::stream! {
                    let listener = net::TcpListener::bind(addr).await?;
                    let mut rx = tx.clone().subscribe();
                    loop {
                        tokio::select! {
                            res = listener.accept() => {
                                yield res.map(|(s, _)| s);
                            }
                            peer_message = rx.recv() => {
                                let mut peer_message = peer_message.unwrap();
                                peer_message.randomly_corrupt(rng.clone());
                                process_peer_message(peer_message, &signing_key, tx.clone()).await
                            }
                        }
                    }
                }))
                .serve(Shared::new(greeter))
                .await
                .unwrap();

                Ok(())
            }
            .instrument(tracing::span!(tracing::Level::INFO, "server", i))
        })
    }

    for i in 0..num_client {
        let client_name = format!("client{}", i);
        let server_url = format!("http://server{}:{}", i, 9999 - (i as u32));
        let mut rng = SmallRng::from_entropy();
        sim.client(
            client_name,
            async move {
                let ch = Endpoint::new(server_url)?
                    .connect_with_connector(connector::connector())
                    .await?;
                let mut greeter_client = MulticastClient::new(ch);

                let rand_string = (0..10)
                    .map(|_| rng.sample(rand::distributions::Alphanumeric))
                    .collect::<Vec<_>>();
                let request = Request::new(RmcMessage {
                    message: String::from_utf8_lossy(&rand_string).to_string(),
                });
                let res = greeter_client.send(request).await?;

                tracing::info!("Client {:?} got response: {:?}", i, res);

                Ok(())
            }
            .instrument(tracing::span!(tracing::Level::INFO, "client", i)),
        );
    }

    sim
}

fn generate_multicast_server(
    tx: broadcast::Sender<RmcPeerMessage>,
    signing_key: &[u8; 32],
) -> (SigningKey, MulticastServer<MyMulticaster<SigningKey>>) {
    let signing_key = SigningKey::from_bytes(signing_key);
    let verifying_key = signing_key.verifying_key();
    (
        signing_key.clone(),
        MulticastServer::new(MyMulticaster {
            sender: tx.clone(),
            signing_key,
            verifying_key,
        }),
    )
}

async fn process_peer_message(
    mut peer_message: RmcPeerMessage,
    signing_key: &SigningKey,
    tx: broadcast::Sender<RmcPeerMessage>,
) {
    let digest = peer_message.data.clone();
    if peer_message.signatures.len() != peer_message.verifying_keys.len() {
        tracing::error!("mismatched signature and verifying key lengths!");
        return;
    }
    let mut signatures = peer_message.signatures;
    let mut verifying_keys = peer_message.verifying_keys.clone();
    for (key, sig) in peer_message.verifying_keys.iter().zip(signatures.iter()) {
        if !key.verify(&digest, sig).is_ok() {
            tracing::error!("mismatched signature for verifying key!");
            return;
        }
    }

    let my_sig = signing_key.sign(&digest);
    signatures.push(my_sig);
    verifying_keys.push(signing_key.verifying_key());
    peer_message.signatures = signatures.clone();
    peer_message.verifying_keys = verifying_keys.clone();

    if (signatures.len() as u64) < RMC_THRESHOLD {
        match tx.send(peer_message) {
            Ok(_) => {}
            Err(_) => tracing::info!("Failed to send message to peers"),
        }
    }

    let comma_separated: String = signatures
        .iter()
        .map(|sig| sig.to_vec())
        .map(|bytes| {
            bytes
                .iter()
                .map(|&x| x.to_string())
                .collect::<Vec<String>>()
                .join(",")
        })
        .collect::<Vec<String>>()
        .join(",");
    let path = OsString::from_vec(hex::encode(&digest.clone()).into());
    if !turmoil::has(path.clone()).await {
        let _ = turmoil::append(path, comma_separated.into()).await;
    } else {
        let _ = turmoil::write(path, comma_separated.into()).await;
    }
}

#[derive(Clone, Debug)]
struct RmcPeerMessage {
    data: Vec<u8>,
    signatures: Vec<Signature>,
    verifying_keys: Vec<VerifyingKey>,
}

impl TurmoilMessage for RmcPeerMessage {
    fn randomly_corrupt(&mut self, mut rng: impl RngCore + 'static) {
        let failure = rng.gen::<bool>();
        if failure {
            let fail_data = rng.gen::<bool>();
            let fail_sig = rng.gen::<bool>();
            if fail_data {
                let mut rand_data = vec![0u8; self.data.len()];
                rng.fill_bytes(&mut rand_data);
                self.data = rand_data;
            }
            if fail_sig {
                let num_to_mutate = rng.gen_range(0..self.signatures.len() + 1);

                for _ in 0..num_to_mutate {
                    let index = rng.gen_range(0..self.signatures.len());
                    let mut rand_data = vec![0u8; 64];
                    rng.fill_bytes(&mut rand_data);
                    let rand_data: &[u8; 64] = rand_data.as_slice().try_into().unwrap();
                    let rand_sig = Signature::from_bytes(rand_data);
                    self.signatures[index] = rand_sig;
                }
            }
        }
    }
}

pub struct MyMulticaster<S>
where
    S: Signer<Signature>,
{
    sender: broadcast::Sender<RmcPeerMessage>,
    pub signing_key: S,
    verifying_key: VerifyingKey,
}

#[tonic::async_trait]
impl<S> Multicast for MyMulticaster<S>
where
    S: 'static + Signer<Signature> + Send + Sync,
{
    async fn send(&self, request: Request<RmcMessage>) -> Result<Response<RmcResponse>, Status> {
        let message = request.into_inner();
        tracing::info!("Received message: {}", message.message);

        let mut hasher = Sha512::new();
        hasher.update(message.clone().message);
        let hash = hasher.finalize();
        let sig = self.signing_key.sign(&hash);

        let peer_message = RmcPeerMessage {
            data: hash.to_vec(),
            signatures: Vec::from([sig]),
            verifying_keys: Vec::from([self.verifying_key]),
        };

        // Send the message to all peers.
        match self.sender.send(peer_message) {
            Ok(_) => {}
            Err(_) => tracing::info!("Failed to send message to peers"),
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
