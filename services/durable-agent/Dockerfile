FROM node:22-alpine@sha256:fe4f6d74e20370acb04ad889077d14d0f64a9c8ea6a42bc60cddf1e844699914
WORKDIR /app
COPY package.json package-lock.json ./
RUN npm ci --omit=dev --ignore-scripts --no-audit --no-fund && npm cache clean --force \
    && addgroup -g 10001 foyer && adduser -D -u 10001 -G foyer foyer \
    && mkdir -p /data/agent && chown -R 10001:10001 /data
COPY src ./src
USER 10001:10001
ENV FOYER_AGENT_DATA_DIR=/data/agent FOYER_AGENT_HOST=127.0.0.1 FOYER_AGENT_PORT=3584
CMD ["node", "src/main.mjs"]
