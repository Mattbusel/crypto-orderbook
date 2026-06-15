# ── Stage 1: Build ────────────────────────────────────────────────────────────
# We use the official Rust image to compile. The result is a statically linked
# binary that we copy into a minimal runtime image. This keeps the final image
# small and removes the compiler and all build tools from production.
FROM rust:1.78-slim AS builder

WORKDIR /app

# Copy dependency manifests first and build them separately.
# Docker caches each layer. If only src/ changes (not Cargo.toml),
# the dependency compilation layer is reused and builds stay fast.
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release
RUN rm src/main.rs

# Now copy the real source and compile the final binary.
COPY src ./src
RUN touch src/main.rs && cargo build --release

# ── Stage 2: Runtime ──────────────────────────────────────────────────────────
# Distroless contains only the minimum runtime libraries.
# No shell, no package manager, no attack surface.
FROM gcr.io/distroless/cc-debian12

WORKDIR /app
COPY --from=builder /app/target/release/crypto-orderbook .

# Default environment. Override these when running the container.
ENV SYMBOL=BTCUSDT
ENV API_PORT=3000
ENV RUST_LOG=info

EXPOSE 3000

CMD ["./crypto-orderbook"]
