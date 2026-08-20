FROM rust:1.95-slim AS builder
WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates

RUN cargo build --release -p light-factory-server

FROM debian:bookworm-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates libgcc-s1 \
 && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/light-factory-server /usr/local/bin/light-factory-server

ENV ADDR=0.0.0.0:8080
EXPOSE 8080
CMD ["light-factory-server"]
