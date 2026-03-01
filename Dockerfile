# Use Rust official image
FROM rust:1.75 as builder

WORKDIR /usr/src/shadowmixer

# Copy manifests first for caching
COPY Cargo.toml Cargo.lock ./
# Create a dummy main.rs to build dependencies
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release
RUN rm src/main.rs

# Copy source code
COPY src ./src

# Build the actual application
RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim

WORKDIR /app

# Install OpenSSL (needed for reqwest/HTTPS)
RUN apt-get update && apt-get install -y libssl-dev ca-certificates && rm -rf /var/lib/apt/lists/*

COPY --from=builder /usr/src/shadowmixer/target/release/shadowmixer ./shadowmixer

CMD ["./shadowmixer"]
