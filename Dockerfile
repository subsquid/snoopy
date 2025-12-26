# See https://www.lpalmieri.com/posts/fast-rust-docker-builds/#cargo-chef for explanation
FROM --platform=$BUILDPLATFORM lukemathwalker/cargo-chef:latest-rust-1.89-slim AS chef
WORKDIR /app


FROM chef AS planner
COPY Cargo.toml .
COPY Cargo.lock .
COPY src ./src
COPY abi ./abi
RUN cargo chef prepare --recipe-path recipe.json


FROM chef AS builder
RUN apt-get update && apt-get install protobuf-compiler pkg-config libssl-dev build-essential  -y

COPY --from=planner /app/recipe.json recipe.json
RUN --mount=type=ssh cargo chef cook --release --recipe-path recipe.json

COPY Cargo.toml .
COPY Cargo.lock .
COPY src ./src
COPY abi ./abi
RUN --mount=type=ssh cargo build --release


FROM chef AS snoopy
COPY prove-query-result-program /app/prove-query-result-program
COPY static /app/static
COPY templates /app/templates
COPY --from=builder /app/target/release/snoopy /app/snoopy
EXPOSE 8000
ENV ROCKET_ADDRESS=0.0.0.0

ENTRYPOINT ["/app/snoopy"]
