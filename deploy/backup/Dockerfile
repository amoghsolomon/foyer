FROM debian:bookworm-slim

# Pinned backup image: PostgreSQL 18 client (matches pgvector/pgvector:pg18),
# age 1.2.1, zstd, and Python 3 for generic S3 SigV4 transfer.
ARG DEBIAN_FRONTEND=noninteractive
ARG AGE_VERSION=1.2.1
ARG AGE_ARCHIVE=age-v1.2.1-linux-amd64.tar.gz
ARG AGE_SHA256=7df45a6cc87d4da11cc03a539a7470c15b1041ab2b396af088fe9990f7c79d50
ARG POSTGRES_CLIENT_MAJOR=18

RUN apt-get update \
    && apt-get install --yes --no-install-recommends \
        ca-certificates \
        curl \
        gnupg \
        jq \
        python3 \
        tar \
        zstd \
    && install -d /usr/share/postgresql-common/pgdg \
    && curl -fsSL https://www.postgresql.org/media/keys/ACCC4CF8.asc \
        | gpg --dearmor -o /usr/share/postgresql-common/pgdg/apt.postgresql.org.gpg \
    && echo "deb [signed-by=/usr/share/postgresql-common/pgdg/apt.postgresql.org.gpg] https://apt.postgresql.org/pub/repos/apt bookworm-pgdg main" \
        > /etc/apt/sources.list.d/pgdg.list \
    && apt-get update \
    && apt-get install --yes --no-install-recommends "postgresql-client-${POSTGRES_CLIENT_MAJOR}" \
    && curl -fsSL -o /tmp/age.tgz \
        "https://github.com/FiloSottile/age/releases/download/v${AGE_VERSION}/${AGE_ARCHIVE}" \
    && echo "${AGE_SHA256}  /tmp/age.tgz" | sha256sum -c - \
    && tar -xzf /tmp/age.tgz -C /tmp \
    && install -m 0755 /tmp/age/age /usr/local/bin/age \
    && install -m 0755 /tmp/age/age-keygen /usr/local/bin/age-keygen \
    && rm -rf /tmp/age /tmp/age.tgz /var/lib/apt/lists/*

WORKDIR /backup
COPY VERSION /backup/VERSION
COPY scripts/lib.sh scripts/s3.py scripts/foyer-backup /backup/scripts/
RUN chmod 0755 /backup/scripts/foyer-backup /backup/scripts/s3.py \
    && chmod 0644 /backup/scripts/lib.sh /backup/VERSION

# No Docker socket. Source volumes are mounted read-only by the host wrapper.
ENV PATH="/backup/scripts:${PATH}"
ENTRYPOINT ["/backup/scripts/foyer-backup"]
CMD ["help"]
