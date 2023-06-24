//! Create a custom data transport to use with a Provider.

use async_trait::async_trait;
use ethers::{
    prelude::{Http, JsonRpcClient, ProviderError, RetryClientError, RpcError},
    providers::{HttpRateLimitRetryPolicy, RetryClient, RetryClientBuilder},
};
use prometheus::{histogram_opts, Histogram, IntCounter, IntCounterVec, Opts, Registry};
use serde::{de::DeserializeOwned, Serialize};
use std::sync::Arc;
use std::time::Duration;
use std::{fmt::Debug, str::FromStr};
use thiserror::Error;

/// First we must create an error type, and implement [`From`] for
/// [`ProviderError`].
///
/// Here we are using [`thiserror`](https://docs.rs/thiserror) to wrap
/// [`RetryClientError`].
///
/// This also provides a conversion implementation ([`From`]) for both, so we
/// can use the [question mark operator](https://doc.rust-lang.org/rust-by-example/std/result/question_mark.html)
/// later on in our implementations.
#[derive(Debug, Error)]
pub enum MeasuredJsonRpcError {
    #[error(transparent)]
    Http(#[from] RetryClientError),
}

/// In order to use our `InstrumentedJsonRpcError` in the RPC client, we have to implement
/// this trait.
///
/// [`RpcError`] helps other parts off the stack get access to common provider
/// error cases. For example, any RPC connection may have a `serde_json` error,
/// so we want to make those easily accessible, so we implement
/// `as_serde_error()`
///
/// In addition, RPC requests may return JSON errors from the node, describing
/// why the request failed. In order to make these accessible, we implement
/// `as_error_response()`.
impl RpcError for MeasuredJsonRpcError {
    fn as_error_response(&self) -> Option<&ethers::providers::JsonRpcError> {
        match self {
            MeasuredJsonRpcError::Http(e) => e.as_error_response(),
        }
    }

    fn as_serde_error(&self) -> Option<&serde_json::Error> {
        match self {
            MeasuredJsonRpcError::Http(RetryClientError::SerdeJson(err)) => Some(err),
            _ => None,
        }
    }
}

/// This implementation helps us convert our Error to the library's
/// [`ProviderError`] so that we can use the `?` operator
impl From<MeasuredJsonRpcError> for ProviderError {
    fn from(value: MeasuredJsonRpcError) -> Self {
        Self::JsonRpcClientError(Box::new(value))
    }
}

/// Define a struct to hold the metrics we want to track. For this example, we will track:
/// - `request_total`: the total number of requests made to the RPC URL
/// - `request_latency`: the time taken for the RPC URL to respond
/// - `request_errors`: the total number of errors from the RPC URL
#[derive(Clone, Debug)]
pub struct Metrics {
    request_total: IntCounter,
    request_latency: Histogram,
    request_errors: IntCounterVec,
}

/// We implement a constructor method for our metrics, which will initialize the metrics and
/// register them with the provided [`Registry`].
impl Metrics {
    fn new(registry: &Registry) -> Self {
        let request_total =
            IntCounter::new("request_total", "Total number of requests made to RPC URL")
                .expect("could not create request_total counter");
        let request_latency = Histogram::with_opts(histogram_opts!(
            "request_latency",
            "The time taken for RPC URL to respond"
        ))
        .expect("could not create request_latency histogram");
        let request_errors = IntCounterVec::new(
            Opts::new("request_errors", "Total number of errors from RPC URL"),
            &["code"],
        )
        .expect("could not create request_errors counter");
        registry
            .register(Box::new(request_total.clone()))
            .expect("could not register request_total counter");
        registry
            .register(Box::new(request_latency.clone()))
            .expect("could not register request_latency histogram");
        registry
            .register(Box::new(request_errors.clone()))
            .expect("could not register request_errors counter");

        Self {
            request_total,
            request_latency,
            request_errors,
        }
    }
}

/// Next, we create our transport type, which in this case will be a struct that contains
/// only [`RetryClient<Http>`] and our metrics.
#[derive(Clone, Debug)]
pub struct MeasuredJsonRpc {
    client: Arc<RetryClient<Http>>,
    metrics: Metrics,
}

// We implement a convenience "constructor" method, to easily initialize the transport.
// This will initialize the underlying http transport, setup the retry client, then wrap it in our custom type.
// It will also bind the metrics to the registry.
impl MeasuredJsonRpc {
    pub fn new(url: impl Into<String>, registry: &Registry) -> Self {
        let http = Http::from_str(url.into().as_str()).expect("could not initialize http");

        let client = Arc::new(
            RetryClientBuilder::default()
                .rate_limit_retries(10)
                .timeout_retries(3)
                .initial_backoff(Duration::from_millis(500))
                .build(http, Box::<HttpRateLimitRetryPolicy>::default()),
        );

        let metrics = Metrics::new(registry);
        Self { client, metrics }
    }
}

// Next, the most important step: implement [`JsonRpcClient`].
//
// For this implementation, we simply delegate to the wrapped transport and return the
// result.
//
// Note that we are using [`async-trait`](https://docs.rs/async-trait) for asynchronous
// functions in traits, as this is not yet supported in stable Rust; see:
// <https://blog.rust-lang.org/inside-rust/2022/11/17/async-fn-in-trait-nightly.html>
#[async_trait]
impl JsonRpcClient for MeasuredJsonRpc {
    type Error = MeasuredJsonRpcError;

    async fn request<T, R>(&self, method: &str, params: T) -> Result<R, Self::Error>
    where
        T: Debug + Serialize + Send + Sync,
        R: DeserializeOwned + Send,
    {
        log::trace!("request: method: {}, params: {:?}", method, params);
        let timer = self.metrics.request_latency.start_timer();
        let res = self.client.request(method, params).await;
        timer.observe_duration();
        self.metrics.request_total.inc();

        match &res {
            Ok(_) => {}
            Err(error) => {
                // track error by status code, etc.
                match error {
                    RetryClientError::ProviderError(err) => {
                        // check for json rpc error codes
                        if let Some(code) = err.as_error_response().map(|e| e.code) {
                            log::debug!("json rpc error: {}", code);
                            self.metrics
                                .request_errors
                                .with_label_values(&[&code.to_string()])
                                .inc();
                        } else if let ProviderError::HTTPError(_err) = err {
                            // check for http error codes
                            if let Some(status) = _err.status() {
                                log::debug!("http error: {}", status);
                                self.metrics
                                    .request_errors
                                    .with_label_values(&[&status.as_str()])
                                    .inc();
                            }
                        } else {
                            log::debug!("unknown error");
                            self.metrics
                                .request_errors
                                .with_label_values(&["unknown"])
                                .inc();
                        }
                    }
                    RetryClientError::SerdeJson(_) => {
                        log::debug!("serde json error");
                        self.metrics
                            .request_errors
                            .with_label_values(&["serde_json"])
                            .inc();
                    }
                    &RetryClientError::TimeoutError => {
                        log::debug!("timeout error");
                        self.metrics
                            .request_errors
                            .with_label_values(&["timeout"])
                            .inc();
                    }
                }
            }
        }

        res.map_err(Into::into)
    }
}
