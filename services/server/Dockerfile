FROM rust:1.97.1-bookworm AS builder

WORKDIR /build
COPY services/server/Cargo.toml services/server/Cargo.lock ./
COPY services/server/src ./src
COPY services/server/migrations ./migrations
RUN cargo build --locked --release --bins

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
RUN useradd --create-home --uid 10001 foyer

COPY --from=builder /build/target/release/foyer-server /usr/local/bin/foyer-server
COPY --from=builder /build/target/release/foyer-admin /usr/local/bin/foyer-admin

USER foyer
ENV FOYER_SERVER_BIND=0.0.0.0:3583
ENV FOYER_SERVER_HEALTH_ADDR=127.0.0.1:3583
EXPOSE 3583

ENTRYPOINT ["foyer-server"]
