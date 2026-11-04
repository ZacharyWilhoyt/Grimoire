FROM node:24-bookworm-slim AS web
WORKDIR /build/web
COPY web/package*.json ./
RUN npm ci
COPY web/ ./
RUN npm run build

FROM rust:1-bookworm AS backend
WORKDIR /build/backend
COPY backend/ ./
RUN CARGO_BUILD_JOBS=2 cargo build --locked --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates ffmpeg fonts-dejavu-core openssl && rm -rf /var/lib/apt/lists/* && useradd --uid 10001 --create-home grimoire
WORKDIR /app
COPY --from=backend /build/backend/target/release/grimoire-api /usr/local/bin/grimoire-api
COPY --from=web /build/web/dist /app/web
RUN mkdir -p /app/data && chown -R 10001:10001 /app
USER 10001:10001
ENV GRIMOIRE_WEB_ROOT=/app/web GRIMOIRE_DATA_DIR=/app/data
ENTRYPOINT ["grimoire-api"]
