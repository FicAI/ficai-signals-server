FROM lukemathwalker/cargo-chef:0.1.35-rust-1.61 AS chef
WORKDIR app


FROM chef AS planner
COPY src src/
COPY Cargo.* ./
RUN cargo chef prepare --recipe-path recipe.json


FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
# Build dependencies - separately for Docker layer caching
RUN cargo chef cook --release --recipe-path recipe.json
# Build application
COPY src src/
COPY Cargo.* ./
RUN cargo build --release --bin ficai-signals-server


FROM debian:bullseye-slim AS runtime
WORKDIR app
COPY --from=builder /app/target/release/ficai-signals-server /usr/local/bin
ENTRYPOINT ["/usr/local/bin/ficai-signals-server"]
