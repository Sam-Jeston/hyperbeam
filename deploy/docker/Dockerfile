# syntax=docker/dockerfile:1
# Builds the control plane and the runner.
#
#   docker build -f deploy/docker/Dockerfile --target server -t noemune/server .
#   docker build -f deploy/docker/Dockerfile --target runner -t noemune/runner .

FROM oven/bun:1 AS web
WORKDIR /src/web
COPY web/package.json web/bun.lock* ./
RUN bun install --frozen-lockfile
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
    cargo build --release --locked -p noemune-server -p noemune-cli \
    # The runner is copied into arbitrary harness images, so it is fully static.
    && cargo build --release --locked -p noemune-runner --target "$(uname -m)-unknown-linux-musl" \
    && mkdir -p /out \
    && cp target/release/noemune-server target/release/noemune /out/ \
    && cp "target/$(uname -m)-unknown-linux-musl/release/noemune-runner" /out/

FROM debian:bookworm-slim AS server
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/* \
    && useradd --uid 10001 --create-home noemune
COPY --from=build /out/noemune-server /out/noemune /out/noemune-runner /usr/local/bin/
USER 10001
EXPOSE 8080 8081
ENTRYPOINT ["noemune-server"]
CMD ["serve"]

# Nothing but the static binary: an init container copies it into the sandbox.
FROM scratch AS runner
COPY --from=build /out/noemune-runner /noemune-runner
USER 1000
ENTRYPOINT ["/noemune-runner"]
