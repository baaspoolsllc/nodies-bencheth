<a href="https://nodies.app?ref=bencheth">
  <img src="./docs/imgs/nodies.png" alt="Nodies DLB" width="100%" />
  <br/>
  <br/>
</a>

# 🏋️‍♂️ BenchETH - Measure your RPC provider's performance

BenchETH is a simple benchmark tool for measuring the performance of Ethereum JSON-RPC servers. It is only interested in latest block height and the latest transactions.

## About

BenchETH utilizes [`ethers-rs`](https://github.com/gakonst/ethers-rs) to connect to Ethereum JSON-RPC servers and measure their performance. It is written in Rust and it's main goal is to be a simple and easy to use benchmarking tool. It uses a custom JSON-RPC client to connect to the Ethereum node and measure the performance.

### Metrics

A number of Prometheus metrics are exported by BenchETH on port 3030. The metrics are:

- `request_total`: Total number of requests made to RPC URL
- `request_latency`: The time taken for RPC URL to respond
- `request_errors`: Total number of errors from RPC URL

### Configuration

BenchETH is configured via environment variables. The most important is `RPC_URL`, which is the URL of the RPC server to connect to. The other environment variables can be found in the [`.env.example`](.env.example) file.

- `RPC_URL`: The URL of the RPC server to connect to.

### Running

TODO

### Deploying

For a spicier experience 🌶️, you can deploy BenchETH to your own DigitalOcean droplets 🌊.

#### Grafana Cloud

Monitoring is done via [Grafana cloud](https://grafana.com/), sign up for an account and then plugin your environment variables in the
[`.env.example`](.env.example) file unless you don't care about the results.

#### DigitalOcean

Sign up for an account at [digitalocean.com](https://digitalocean.com), upload an SSH key, and then run:

> **Warning**: This will create droplets in your account and you will be charged for them. Make sure to destroy them after you're done.

The benchmark uses a $7/month droplet and deploys to 6 regions across the world.

```bash
export SSH_KEY="~/.ssh/id_rsa" # path to the private key you uploaded
./scripts/deploy_doctl.sh
# press enter to confirm
```

Then, it will deploy to regions all across the world. It takes a few minutes to build and deploy the benchmark.

You can then run `./scripts/deploy_doctl.sh` to destroy all the droplets.
