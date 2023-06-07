mod measured_json_rpc_client;
mod metrics_server;
use measured_json_rpc_client::MeasuredJsonRpc;

use chrono::{DateTime, NaiveDateTime, Utc};
use dotenv::dotenv;
use ethers::prelude::*;
use prometheus::Registry;
use reqwest::Url;
use tokio::time;

use std::collections::HashMap;
use std::env;
use std::sync::Arc;
use std::time::Duration;

#[warn(unreachable_code)]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // if .env exists load it
    dotenv().ok();
    env_logger::init();

    // get the RPC_URL from the environment
    let rpc_url = env::var("RPC_URL").expect("Invalid RPC_URL");
    let rpc_url = Url::parse(&rpc_url).expect("Invalid RPC_URL");
    let rpc_host = rpc_url.host_str().unwrap();

    // get geo region
    let geo_region = get_geo_region().await;

    log::info!(
        "[😛-bencheth][🗺️-{}] ➡️ {}:  {:?}",
        geo_region,
        rpc_host,
        env!("CARGO_PKG_VERSION")
    );

    let block_number_gauge = prometheus::Gauge::new("block_number", "Block number").unwrap();

    let mut labels = HashMap::new();
    labels.insert("rpc".to_string(), rpc_host.to_string());
    labels.insert("geo".to_string(), geo_region.to_string());
    let registry = Registry::new_custom(None, Some(labels)).expect("Failed to create registry");

    registry
        .register(Box::new(block_number_gauge.clone()))
        .unwrap();

    let registry_for_spawn = registry.clone();

    tokio::spawn(async move {
        crate::metrics_server::start_metrics_server(registry_for_spawn).await;
    });

    let transport = MeasuredJsonRpc::new(rpc_url.as_str(), &registry);
    let mut provider = Provider::new(transport);

    // set the polling interval to 500ms
    provider.set_interval(Duration::from_millis(500));

    let provider = Arc::new(provider);

    // This uses eth_getFilterChanges underneath the hood which does not work well with RPC providers that load balance 😿
    // let mut stream = provider
    //     .watch_blocks()
    //     .await
    //     .expect("Failed to watch blocks");

    // check for new blocks every 500ms
    let mut interval = time::interval(Duration::from_millis(500));

    loop {
        interval.tick().await;

        let mut curr_block_height = match provider.get_block_number().await {
            Ok(b) => b,
            Err(e) => {
                log::warn!("Failed to get block number: {:?}", e);
                continue;
            }
        };

        if curr_block_height == U64::zero() {
            continue;
        }

        log::info!("Current block height: {}", curr_block_height);
        block_number_gauge.set(curr_block_height.as_u64() as f64);

        loop {
            interval.tick().await;

            let latest_block_height = match provider.get_block_number().await {
                Ok(b) => b,
                Err(e) => {
                    log::warn!("Failed to get block number: {:?}", e);
                    continue;
                }
            };

            if latest_block_height == curr_block_height {
                continue;
            }

            if latest_block_height < curr_block_height {
                log::warn!(
                    "Latest block height {} is lower than current block height {}",
                    latest_block_height,
                    curr_block_height
                );
                continue;
            }

            log::info!(
                "Current block height: {} ({} new blocks)",
                latest_block_height,
                latest_block_height - curr_block_height
            );

            while curr_block_height < latest_block_height {
                curr_block_height += U64::one();
                let block = match provider.get_block(curr_block_height).await {
                    Ok(b) => b,
                    Err(e) => {
                        log::warn!("Failed to get block {:?}: {:?}", curr_block_height, e);
                        continue;
                    }
                };

                if block.is_none() {
                    continue;
                }
                let block = block.unwrap();

                block_number_gauge.set(block.number.unwrap().as_u64() as f64);

                let timestamp = DateTime::<Utc>::from_utc(
                    NaiveDateTime::from_timestamp_opt(block.timestamp.as_u64() as i64, 0)
                        .expect("Invalid block timestamp"),
                    Utc,
                );

                let transactions = tokio_stream::iter(block.transactions)
                    .map(|tx_hsh| {
                        let tx_provider = provider.clone();
                        async move {
                            get_transaction(&tx_hsh, tx_provider).await;
                        }
                    })
                    .buffer_unordered(num_cpus::get())
                    .collect::<Vec<_>>()
                    .await;

                log::info!(
                    "New block height {} at {} with timestamp {} with {} txs found after {}.",
                    block.number.unwrap().as_u64(),
                    block.hash.unwrap(),
                    timestamp,
                    transactions.len(),
                    Utc::now() - timestamp
                );
            }
        }
    }
}

async fn get_geo_region() -> String {
    let region = reqwest::get("https://ipinfo.io/json")
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    let region: serde_json::Value = serde_json::from_str(&region).unwrap();
    let country = region["country"].as_str().unwrap();
    let region = region["region"].as_str().unwrap();
    format!("{}-{}", country, region)
}

async fn get_transaction(tx_hsh: &H256, provider: Arc<Provider<MeasuredJsonRpc>>) {
    let tx = match provider.get_transaction(*tx_hsh).await {
        Ok(tx) => tx,
        Err(e) => {
            log::warn!("Failed to get transaction {:?}: {:?}", tx_hsh, e);
            return;
        }
    };

    if tx.is_none() {
        return;
    }

    let tx = tx.unwrap();
    log::trace!("Transaction {} found at {}", tx.hash, Utc::now());
}
