//! Create a custom data transport to use with a Provider.

use async_trait::async_trait;
use ethers::{
    prelude::{Http, JsonRpcClient, ProviderError, RetryClientError, RpcError},
    providers::{
        HttpClientError, HttpRateLimitRetryPolicy, JsonRpcError, RetryClient, RetryClientBuilder,
        RetryPolicy,
    },
};
use prometheus::{histogram_opts, Histogram, IntCounter, IntCounterVec, Opts, Registry};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
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
        registry
            .register(Box::new(request_total.clone()))
            .expect("could not register request_total counter");
        registry
            .register(Box::new(request_latency.clone()))
            .expect("could not register request_latency histogram");
        Self {
            request_total,
            request_latency,
        }
    }
}

/// Create a measured retry policy that will track the number of errors from the RPC URL.
#[derive(Debug)]
pub struct MeasuredHttpRateLimitRetryPolicy {
    request_errors: Arc<IntCounterVec>,
    default_policy: HttpRateLimitRetryPolicy,
}

impl MeasuredHttpRateLimitRetryPolicy {
    pub fn new(registry: &Registry) -> Self {
        let request_errors = IntCounterVec::new(
            Opts::new("request_errors", "Total number of errors from RPC URL"),
            &["code"],
        )
        .expect("could not create request_errors counter");

        registry
            .register(Box::new(request_errors.clone()))
            .expect("could not register request_errors counter");

        Self {
            request_errors: Arc::new(request_errors),
            default_policy: HttpRateLimitRetryPolicy::default(),
        }
    }
}

/// We implement the [`HttpRateLimitRetryPolicy`] trait for our measured retry policy.
/// This will allow us to use our custom retry policy with the [`RetryClient`].
/// We will simply increment the counter for the error code, then return the default
/// retry policy.
impl RetryPolicy<HttpClientError> for MeasuredHttpRateLimitRetryPolicy {
    fn should_retry(&self, error: &HttpClientError) -> bool {
        fn should_retry_json_rpc_error(err: &JsonRpcError, req_errs: Arc<IntCounterVec>) -> bool {
            let JsonRpcError { code, message, .. } = err;

            log::debug!("JSON RPC error: code={}, message={}", code, message);
            req_errs.with_label_values(&[&code.to_string()]).inc();

            // alchemy throws it this way
            if *code == 429 {
                return true;
            }

            // This is an infura error code for `exceeded project rate limit`
            if *code == -32005 {
                return true;
            }

            // alternative alchemy error for specific IPs
            if *code == -32016 && message.contains("rate limit") {
                return true;
            }

            match message.as_str() {
                // this is commonly thrown by infura and is apparently a load balancer issue, see also <https://github.com/MetaMask/metamask-extension/issues/7234>
                "header not found" => true,
                // also thrown by infura if out of budget for the day and ratelimited
                "daily request count exceeded, request rate limited" => true,
                _ => false,
            }
        }

        match error {
            HttpClientError::ReqwestError(err) => {
                let status = err
                    .status()
                    .map(|s| s.as_u16().to_string())
                    .unwrap_or_default();
                log::debug!("Reqwest error: {:?}", err);
                self.request_errors.with_label_values(&[&status]).inc();
                err.status() == Some(http::StatusCode::TOO_MANY_REQUESTS)
            }
            HttpClientError::JsonRpcError(err) => {
                should_retry_json_rpc_error(err, self.request_errors.clone())
            }
            HttpClientError::SerdeJson { text, .. } => {
                // some providers send invalid JSON RPC in the error case (no `id:u64`), but the
                // text should be a `JsonRpcError`
                #[derive(Deserialize)]
                struct Resp {
                    error: JsonRpcError,
                }

                // log the first 100 chars of the error
                log::debug!("SerdeJSON error: {}", &text);

                if let Ok(resp) = serde_json::from_str::<Resp>(text) {
                    return should_retry_json_rpc_error(&resp.error, self.request_errors.clone());
                }
                self.request_errors.with_label_values(&["unknown"]).inc();
                false
            }
        }
    }

    fn backoff_hint(&self, error: &HttpClientError) -> Option<Duration> {
        self.default_policy.backoff_hint(error)
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
                .build(
                    http,
                    Box::new(MeasuredHttpRateLimitRetryPolicy::new(registry)),
                ),
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
        res.map_err(Into::into)
    }
}
