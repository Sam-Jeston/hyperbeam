# syntax=docker/dockerfile:1
# Builds the control plane and the runner.
#
#   docker build -f deploy/docker/Dockerfile --target server -t hyperbeam/server .
#   docker build -f deploy/docker/Dockerfile --target runner -t hyperbeam/runner .

FROM oven/bun:1 AS web
WORKDIR /src/web
COPY web/package.json web/bun.lock* ./
RUN bun install --frozen-lockfile
COPY brand/ /src/brand/
COPY web/ ./
RUN bun run build

FROM rust:1.88-bookworm AS build
RUN apt-get update && apt-get install -y --no-install-recommends musl-tools && rm -rf /var/lib/apt/lists/* \
    && rustup target add "$(uname -m)-unknown-linux-musl"
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY --from=web /src/web/dist ./web/dist
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p hyperbeam-server -p hyperbeam-cli \
    # The runner is copied into arbitrary harness images, so it is fully static.
    && cargo build --release --locked -p hyperbeam-runner --target "$(uname -m)-unknown-linux-musl" \
    && mkdir -p /out \
    && cp target/release/hyperbeam-server target/release/hyperbeam /out/ \
    && cp "target/$(uname -m)-unknown-linux-musl/release/hyperbeam-runner" /out/

FROM debian:bookworm-slim AS server
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/* \
    && useradd --uid 10001 --create-home hyperbeam
COPY --from=build /out/hyperbeam-server /out/hyperbeam /out/hyperbeam-runner /usr/local/bin/
USER 10001
EXPOSE 8080 8081
ENTRYPOINT ["hyperbeam-server"]
CMD ["serve"]

# Nothing but the static binary: an init container copies it into the sandbox.
FROM scratch AS runner
COPY --from=build /out/hyperbeam-runner /hyperbeam-runner
USER 1000
ENTRYPOINT ["/hyperbeam-runner"]
