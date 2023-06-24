use std::env;

use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Response, Server};
use prometheus::{Encoder, Registry, TextEncoder};

pub async fn start_metrics_server(registry: Registry) {
    let make_svc = make_service_fn(|_| {
        let registry = registry.clone();
        async {
            Ok::<_, hyper::Error>(service_fn(move |_req| {
                let metric_families = registry.gather();
                let mut buffer = vec![];
                let encoder = TextEncoder::new();
                encoder.encode(&metric_families, &mut buffer).unwrap();

                let response = Response::builder()
                    .status(200)
                    .body(Body::from(buffer))
                    .unwrap();
                async { Ok::<_, hyper::Error>(response) }
            }))
        }
    });

    // pull port from env or default to 9090
    let port = env::var("METRICS_PORT")
        .unwrap_or_else(|_| "9090".to_string())
        .parse::<u16>()
        .unwrap();
    let addr = ([0, 0, 0, 0], port).into();
    let server = Server::bind(&addr).serve(make_svc);

    if let Err(e) = server.await {
        eprintln!("server error: {}", e);
    }
}
