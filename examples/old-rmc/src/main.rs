use axum::extract::Path;
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use axum::{body::Body, http::Request};
use hyper::server::accept::from_stream;
use hyper::{Client, Server, Uri};
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::sync::broadcast;
use tower::make::Shared;
use tracing::{info_span, Instrument};
use turmoil::{net, Builder};
use ed25519::Signature;

use std::ffi::OsString;

fn main() {
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "info");
    }

    tracing_subscriber::fmt::init();

    let addr0 = (IpAddr::from(Ipv4Addr::UNSPECIFIED), 9997);
    let addr1 = (IpAddr::from(Ipv4Addr::UNSPECIFIED), 9998);

    let mut sim = Builder::new()
        .simulation_duration(Duration::from_secs(60))
        .min_message_latency(Duration::from_millis(1))
        .max_message_latency(Duration::from_millis(5))
        .build();

    let counter_path = "counter.txt";

    let (tx, rx) = broadcast::channel::<RMCMessage>(1);

    sim.host("server0", move || {
        let counter_value = Arc::new(Mutex::new(0));
        let router = Router::new().route(
            "/greet/:name",
            get(move |Path(name): Path<String>| async move {
                let mut counter = counter_value.lock().await;
                *counter += 1;
                let ctr_os: OsString = counter_path.to_string().into();
                let _ = turmoil::fs::file_system::append(
                    ctr_os.clone(),
                    counter.to_string().into_bytes(),
                ).await;
                if let Ok(val) = turmoil::read(ctr_os.clone()).await {
                    println!("value in file is {:?}", val);
                } else {
                    panic!("Failed to read value from file");
                }
                format!("Hello {name} counter is {counter}!")
            }),
        );

        async move {
            Server::builder(from_stream(async_stream::stream! {
                let listener = net::TcpListener::bind(addr0).await?;
                loop {
                    yield listener.accept().await.map(|(s, _)| s);
                }
            }))
            .serve(Shared::new(router))
            .await
            .unwrap();

            Ok(())
        }
        .instrument(info_span!("server0"))
    });

    sim.host("server1", move || {
        let counter_value = Arc::new(Mutex::new(0));
        let router = Router::new().route(
            "/greet/:name",
            get(move |Path(name): Path<String>| async move {
                let mut counter = counter_value.lock().await;
                *counter += 1;
                let ctr_os: OsString = counter_path.to_string().into();
                let _ = turmoil::fs::file_system::append(
                    ctr_os.clone(),
                    counter.to_string().into_bytes(),
                ).await;
                if let Ok(val) = turmoil::read(ctr_os.clone()).await {
                    println!("value in file is {:?}", val);
                } else {
                    panic!("Failed to read value from file");
                }
                format!("Hello {name} counter is {counter}!")
            }),
        );

        async move {
            Server::builder(from_stream(async_stream::stream! {
                let listener = net::TcpListener::bind(addr0).await?;
                loop {
                    yield listener.accept().await.map(|(s, _)| s);
                }
            }))
            .serve(Shared::new(router))
            .await
            .unwrap();

            Ok(())
        }
        .instrument(info_span!("server1"))
    });

    sim.client(
        "client",
        async move {
            let client = Client::builder().build(connector::connector());

            // turmoil::hold("client", "server");
            // turmoil::corrupt("client", "server");
            // turmoil::release("client", "server");

            let mut request = Request::new(Body::empty());
            *request.uri_mut() = Uri::from_static("http://server:9997/greet/foo");
            let res = client.request(request).await?;

            let (parts, body) = res.into_parts();
            let body = hyper::body::to_bytes(body).await?;
            let res = Response::from_parts(parts, body);

            tracing::info!("Got response: {:?}", res);

            let mut request = Request::new(Body::empty());
            *request.uri_mut() = Uri::from_static("http://server:9997/greet/foo");
            let res = client.request(request).await?;

            let (parts, body) = res.into_parts();
            let body = hyper::body::to_bytes(body).await?;
            let res = Response::from_parts(parts, body);

            tracing::info!("Got response: {:?}", res);

            Ok(())
        }
        .instrument(info_span!("client")),
    );

    sim.run().unwrap();
}

#[derive(Clone, Debug)]
struct RMCMessage {
    data: Vec<u8>,
    signature: Signature,
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
