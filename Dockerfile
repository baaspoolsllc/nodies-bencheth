# Use an official Rust runtime as a parent image
FROM rust:latest as builder

# Set the working directory in the container to /usr/src/bencheth
WORKDIR /usr/src/bencheth

# Capture dependencies
COPY Cargo.toml Cargo.lock ./

# Build the project in release mode
RUN --mount=type=cache,target=/usr/local/cargo/registry \
  mkdir src && \
  printf "fn main() {println!(\"if you see this, the build broke\")}" > src/main.rs && \
  cargo build --release

# Copy the current directory contents into the container at /usr/src/bencheth
COPY ./src ./src

# A bit of magic here!
# * We're mounting that cache again to use during the build, otherwise it's not present and we'll have to download those again - bad!
# * EOF syntax is neat but not without its drawbacks. We need to `set -e`, otherwise a failing command is going to continue on
# * Rust here is a bit fiddly, so we'll touch the files (even though we copied over them) to force a new build
RUN --mount=type=cache,target=/usr/local/cargo/registry <<EOF
  set -e
  # update timestamps to force a new build
  touch ./src/main.rs
  cargo build --release
EOF

# The second stage of the build uses a smaller image to decrease size
FROM debian:buster-slim

# Install OpenSSL in the slim image
RUN apt-get update && \
  apt-get install -y ca-certificates && \
  rm -rf /var/lib/apt/lists/*

# Copy the binary from the builder stage to the current stage
COPY --from=builder /usr/src/bencheth/target/release/bencheth /usr/local/bin

# Expose port 3030 for the application
EXPOSE 3030

# Run the binary
CMD ["bencheth"]
