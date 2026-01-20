# ==============================================================================
# Dockerfile - WASI Python Host for Raspberry Pi / RevPi
# ==============================================================================
# 
# This builds the Rust host binary for ARM64 and bundles the WASM plugins.
# Requires device passthrough for GPIO/I2C/SPI access.
#
# Build: docker build -t wasi-host .
# Run:   docker-compose up -d
#
# ==============================================================================

FROM rust:1.84-bookworm AS builder

# Install ARM64 cross-compilation tools (for building on x86)
# Skip if building natively on Pi/RevPi
RUN dpkg --add-architecture arm64 && \
    apt-get update && \
    apt-get install -y \
    gcc-aarch64-linux-gnu \
    libc6-dev-arm64-cross \
    pkg-config \
    && rm -rf /var/lib/apt/lists/*

# Set up cross-compilation for ARM64
ENV CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc
ENV PKG_CONFIG_SYSROOT_DIR=/usr/aarch64-linux-gnu

# Create app directory
WORKDIR /app

# Copy source files
COPY host/Cargo.toml host/Cargo.lock ./host/
COPY host/src ./host/src
COPY wit ./wit

# Build the host binary (release mode for ARM64)
WORKDIR /app/host
RUN rustup target add aarch64-unknown-linux-gnu && \
    cargo build --release --target aarch64-unknown-linux-gnu

# ==============================================================================
# Runtime Stage - Minimal image with just the binary and plugins
# ==============================================================================
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && \
    apt-get install -y \
    python3 \
    python3-pip \
    python3-rpi.gpio \
    i2c-tools \
    && rm -rf /var/lib/apt/lists/*

# Install Python libraries for sensor access
RUN pip3 install --break-system-packages \
    adafruit-circuitpython-dht \
    rpi_ws281x \
    bme680

# Create app structure
WORKDIR /app
RUN mkdir -p plugins config scripts

# Copy the built binary
COPY --from=builder /app/host/target/aarch64-unknown-linux-gnu/release/wasi-host ./

# Copy support files
COPY plugins ./plugins/
COPY config ./config/
COPY scripts ./scripts/
COPY wit ./wit/

# Make scripts executable
RUN chmod +x scripts/*.sh scripts/*.py 2>/dev/null || true

# Expose dashboard port
EXPOSE 3000

# Health check
HEALTHCHECK --interval=30s --timeout=10s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:3000/ || exit 1

# Run the host
CMD ["./wasi-host"]
