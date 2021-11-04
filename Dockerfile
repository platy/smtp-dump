FROM lukemathwalker/cargo-chef:latest-rust-1.53.0 AS chef
WORKDIR /app

FROM chef AS planner
COPY . /app
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder 
COPY --from=planner /app/recipe.json recipe.json
# Build dependencies - this is the caching Docker layer!
RUN cargo chef cook --release --recipe-path recipe.json
# Build application
COPY . /app
RUN cargo build --release

# We do not need the Rust toolchain to run the binary!
FROM debian:buster-slim AS runtime
WORKDIR /app/server
COPY --from=builder /app/target/release/smtp-dump /usr/local/bin
ENTRYPOINT ["/usr/local/bin/smtp-dump"]
EXPOSE 25
ENV INBOX_DIR /inbox
ENV TEMP_DIR /tmp
