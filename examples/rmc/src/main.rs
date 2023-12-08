use std::collections::HashMap;
use std::ffi::OsString;
use std::marker::{Send, Sync};
use std::net::{IpAddr, Ipv4Addr};
use std::os::unix::ffi::OsStringExt;
use std::time::Duration;

// Turmoil and server imports
use hyper::server::accept::from_stream;
use hyper::Server;
use tokio::sync::broadcast;
use tokio::sync::broadcast::error::RecvError;
use tonic::transport::Endpoint;
use tonic::Status;
use tonic::{Request, Response};
use tower::make::Shared;
use tracing::Instrument;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::FmtSubscriber;
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

const RMC_THRESHOLD: u64 = 1;

fn main() {
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "info");
    }

    tracing_subscriber::fmt::init();
    // let file_appender = RollingFileAppender::new(
    //     Rotation::NEVER,
    //     "/path/to/logs",
    //     "log",
    // );
    // let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
    // let subscriber = FmtSubscriber::builder()
    //     .pretty()
    //     .with_writer(non_blocking)
    //     .finish();
    // tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");

    let seed: <SmallRng as SeedableRng>::Seed = rand::thread_rng().gen();
    let rng = SmallRng::from_seed(seed);
    tracing::info!("Simulation RNG seed: {:?}", seed);
    let duration = Duration::from_secs(60);
    let mut sim = generate_server_client_configuration(rng, duration);
    sim.run().unwrap();
}

#[test_fuzz::test_fuzz]
fn testy(num_server: u8, num_client: u8) {
    let seed = [num_server; 32];
    let rng = SmallRng::from_seed(seed);
    let mut sim = generate_server_client_config_test(rng, num_server, num_client);
    sim.run().unwrap();
}

fn generate_server_client_config_test(
    mut rng: SmallRng,
    num_server: u8,
    num_client: u8,
) -> Sim<'static> {
    let mut sim = Builder::new().rng(rng.clone()).build();

    let num_server = num_server.min(1).max(3);
    let num_client = num_client.min(1).max(3);

    let (tx, _) = broadcast::channel::<RmcPeerMessage>(16);

    for i in 0..num_server {
        let server_name = format!("server{}", i);
        let port = 9999 - i as u16;
        let addr = (IpAddr::from(Ipv4Addr::UNSPECIFIED), port);
        let tx = tx.clone();
        let (signing_key, greeter) = generate_multicast_server(tx.clone(), &[i; 32]);
        let verifying_key = signing_key.verifying_key();

        let seed: <SmallRng as SeedableRng>::Seed = rng.gen();
        let rng = SmallRng::from_seed(seed);

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
                            msg = rx.recv() => {
                                let mut peer_message: RmcPeerMessage;
                                match msg {
                                    Err(RecvError::Lagged(_)) => {
                                        rx.resubscribe();
                                        continue;
                                    },
                                    Err(_) => {
                                        return;
                                    }
                                    Ok(msg) => {
                                        peer_message = msg;
                                    }
                                }
                                if peer_message.sender == verifying_key {
                                    continue;
                                }
                                peer_message.randomly_corrupt(rng.clone());
                                process_peer_message(peer_message, &signing_key, tx.clone()).await
                            }
                        }
                    }
                }))
                .serve(Shared::new(greeter))
                .await
                .map_err(|err| Box::new(err) as Box<dyn std::error::Error>)
            }
        })
    }

    let seed: <SmallRng as SeedableRng>::Seed = rng.gen();
    let mut rng = SmallRng::from_seed(seed);
    for i in 0..num_client {
        let client_name = format!("client{}", i);
        let server_num = rng.gen_range(0..num_server);
        let server_url = format!("http://server{}:{}", server_num, 9999 - (server_num as u32));
        let rand_string = (0..10)
            .map(|_| rng.sample(rand::distributions::Alphanumeric))
            .collect::<Vec<_>>();
        sim.client(
            client_name,
            async move {
                let ch = Endpoint::new(server_url)?
                    .connect_with_connector(connector::connector())
                    .await?;
                let mut greeter_client = MulticastClient::new(ch);
                let request = Request::new(RmcMessage {
                    message: String::from_utf8_lossy(&rand_string).to_string(),
                });
                greeter_client.send(request).await?;

                let request = Request::new(RmcMessage {
                    message: String::from_utf8_lossy(&rand_string).to_string(),
                });
                let res = greeter_client.get_signatures(request).await?.into_inner();
                decode_get_sig(res.message, rand_string)?;

                Ok(())
            }, // .instrument(tracing::span!(tracing::Level::INFO, "client", i)),
        );
    }

    sim
}

fn generate_server_client_configuration(mut rng: SmallRng, duration: Duration) -> Sim<'static> {
    let mut sim = Builder::new()
        .rng(rng.clone())
        .simulation_duration(duration)
        .build();

    let num_server = rng.gen_range(2..5);
    let num_client = rng.gen_range(2..num_server * 2);

    let (tx, _) = broadcast::channel::<RmcPeerMessage>(16);

    for i in 0..num_server {
        let server_name = format!("server{}", i);
        let port = 9999 - i as u16;
        let addr = (IpAddr::from(Ipv4Addr::UNSPECIFIED), port);
        let tx = tx.clone();
        let (signing_key, greeter) = generate_multicast_server(tx.clone(), &[i; 32]);
        let verifying_key = signing_key.verifying_key();

        let seed: <SmallRng as SeedableRng>::Seed = rand::thread_rng().gen();
        let rng = SmallRng::from_seed(seed);
        tracing::info!("Host {:?} RNG seed: {:?}", i, seed);

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
                            msg = rx.recv() => {
                                let mut peer_message: RmcPeerMessage;
                                match msg {
                                    Err(RecvError::Lagged(n)) => {
                                        tracing::info!("Receiver lagged behind by {} messages", n);
                                        rx.resubscribe();
                                        continue;
                                    },
                                    Err(e) => {
                                        tracing::info!("Receiver error: {:?}", e);
                                        return;
                                    }
                                    Ok(msg) => {
                                        peer_message = msg;
                                    }
                                }
                                if peer_message.sender == verifying_key {
                                    continue;
                                }
                                peer_message.randomly_corrupt(rng.clone());
                                process_peer_message(peer_message, &signing_key, tx.clone()).await
                            }
                        }
                    }
                }))
                .serve(Shared::new(greeter))
                .await
                .map_err(|err| Box::new(err) as Box<dyn std::error::Error>)
            }
            .instrument(tracing::span!(tracing::Level::INFO, "server", i))
        })
    }

    let seed: <SmallRng as SeedableRng>::Seed = rand::thread_rng().gen();
    let mut rng = SmallRng::from_seed(seed);
    tracing::info!("Clients RNG seed: {:?}", seed);
    for i in 0..num_client {
        let client_name = format!("client{}", i);
        let server_num = rng.gen_range(0..num_server);
        let server_url = format!("http://server{}:{}", server_num, 9999 - (server_num as u32));
        let rand_string = (0..10)
            .map(|_| rng.sample(rand::distributions::Alphanumeric))
            .collect::<Vec<_>>();
        sim.client(
            client_name,
            async move {
                let ch = Endpoint::new(server_url)?
                    .connect_with_connector(connector::connector())
                    .await?;
                let mut greeter_client = MulticastClient::new(ch);
                let request = Request::new(RmcMessage {
                    message: String::from_utf8_lossy(&rand_string).to_string(),
                });
                let res = greeter_client.send(request).await?.into_inner();
                tracing::info!("Client {:?} got send response: {:?}", i, res.message);

                let request = Request::new(RmcMessage {
                    message: String::from_utf8_lossy(&rand_string).to_string(),
                });
                let res = greeter_client.get_signatures(request).await?.into_inner();
                tracing::info!("Client {:?} got get signatures response: {:?}", i, res);
                decode_get_sig(res.message, rand_string)?;

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

    let mut unique_pairs: HashMap<VerifyingKey, Signature> = peer_message
        .signatures
        .iter()
        .filter_map(|(sig, vk)| {
            if vk.verify(&digest, sig).is_ok() {
                Some((vk.clone(), sig.clone()))
            } else {
                let vk_bytes = peer_message.sender.to_bytes();
                tracing::info!(
                    "unable to verify current signature from {:?}, proceeding...",
                    hex::encode(vk_bytes)
                );
                None
            }
        })
        .collect();
    if unique_pairs.len() == 0 {
        return;
    }
    let my_sig = signing_key.sign(&digest);
    let my_ver = signing_key.verifying_key();
    unique_pairs.insert(my_ver, my_sig);

    peer_message.signatures = unique_pairs
        .into_iter()
        .map(|(vk, sig)| (sig, vk))
        .collect();
    peer_message.sender = my_ver;

    if (peer_message.signatures.len() as u64) < RMC_THRESHOLD + 1 {
        match tx.send(peer_message.clone()) {
            Ok(_) => {}
            Err(_) => tracing::info!("Failed to send message to peers"),
        }
    }

    let comma_separated: String = peer_message
        .signatures
        .iter()
        .map(|(sig, key)| format!("({}-{})", sig.to_string(), hex::encode(&key.to_bytes())))
        .collect::<Vec<String>>()
        .join(",");
    let path = OsString::from_vec(hex::encode(&digest.clone()).into());
    match turmoil::write(path.clone(), comma_separated.into()).await {
        Ok(_) => {}
        Err(_) => {
            tracing::error!("failed to write!");
        }
    }
}

fn decode_get_sig(s: String, msg: Vec<u8>) -> Result<(), Box<dyn std::error::Error>> {
    let mut hasher = Sha512::new();
    hasher.update(msg);
    let hash = hasher.finalize();

    let pairs: Vec<&str> = s.split(',').collect();
    for pair in pairs {
        let parts: Vec<&str> = pair
            .trim_matches(|c| c == '(' || c == ')')
            .split('-')
            .collect();
        if parts.len() != 2 {
            return Err("Invalid string format".into());
        }

        let sig_bytes = hex::decode(parts[0])?.try_into().unwrap();
        let sig = ed25519_dalek::Signature::from_bytes(&sig_bytes);

        let key_bytes = hex::decode(parts[1])?.try_into().unwrap();
        let key = ed25519_dalek::VerifyingKey::from_bytes(&key_bytes)?;

        // Verify the signature with the verifying key
        key.verify(&hash, &sig)
            .map_err(|_| "Invalid signature")?;
    }

    Ok(())
}

#[derive(Clone, Debug)]
struct RmcPeerMessage {
    data: Vec<u8>,
    signatures: Vec<(Signature, VerifyingKey)>,
    sender: VerifyingKey,
}

impl TurmoilMessage for RmcPeerMessage {
    fn randomly_corrupt(&mut self, mut rng: impl RngCore + 'static) {
        let failure = rng.gen::<u8>();
        if failure < 1 {
            let fail_data = rng.gen::<u8>();
            let fail_sig = rng.gen::<u8>();
            if fail_data < 1 {
                let mut rand_data = vec![0u8; self.data.len()];
                rng.fill_bytes(&mut rand_data);
                self.data = rand_data;
            }
            if fail_sig < 1 {
                let num_to_mutate = rng.gen_range(0..self.signatures.len() + 1);

                for _ in 0..num_to_mutate {
                    let index = rng.gen_range(0..self.signatures.len());
                    let mut rand_data = vec![0u8; 64];
                    rng.fill_bytes(&mut rand_data);
                    let rand_data: &[u8; 64] = rand_data.as_slice().try_into().unwrap();
                    let rand_sig = Signature::from_bytes(rand_data);
                    let (_, ver) = self.signatures[index];
                    self.signatures[index] = (rand_sig, ver);
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
    signing_key: S,
    verifying_key: VerifyingKey,
}

#[tonic::async_trait]
impl<S> Multicast for MyMulticaster<S>
where
    S: 'static + Signer<Signature> + Send + Sync,
{
    async fn send(&self, request: Request<RmcMessage>) -> Result<Response<RmcResponse>, Status> {
        let message = request.into_inner();
        tracing::info!("Send received message: {}", message.message);

        let mut hasher = Sha512::new();
        hasher.update(message.clone().message);
        let hash = hasher.finalize();
        let sig = self.signing_key.sign(&hash);

        let peer_message = RmcPeerMessage {
            data: hash.to_vec(),
            signatures: Vec::from([(sig, self.verifying_key)]),
            sender: self.verifying_key,
        };

        let comma_separated: String = peer_message
            .signatures
            .iter()
            .map(|(sig, key)| format!("({}-{})", sig.to_string(), hex::encode(&key.to_bytes())))
            .collect::<Vec<String>>()
            .join(",");
        let path = OsString::from_vec(hex::encode(&hash).into());
        match turmoil::write(path.clone(), comma_separated.into()).await {
            Ok(_) => {}
            Err(_) => {
                tracing::error!("failed to write!");
            }
        }

        // Send the message to all peers.
        match self.sender.send(peer_message) {
            Ok(_) => {}
            Err(_) => tracing::error!("Failed to send message to peers"),
        }

        Ok(Response::new(RmcResponse {
            message: "Message received".to_string(),
        }))
    }

    async fn get_signatures(
        &self,
        request: Request<RmcMessage>,
    ) -> Result<Response<RmcResponse>, Status> {
        let message = request.into_inner();
        tracing::info!("Get signatures received message: {}", message.message);

        let mut hasher = Sha512::new();
        hasher.update(message.clone().message);
        let hash = hasher.finalize();

        let path = OsString::from_vec(hex::encode(&hash).into());
        let file = turmoil::read(path.clone()).await;
        match file {
            Ok(file) => Ok(Response::new(RmcResponse {
                message: String::from_utf8_lossy(&file).to_string(),
            })),
            Err(e) => Ok(Response::new(RmcResponse {
                message: e.to_string(),
            })),
        }
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
