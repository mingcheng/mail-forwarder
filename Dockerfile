# Stage 1: Dependencies - Cache Rust dependencies separately for faster rebuilds
FROM rust:trixie AS dependencies
LABEL maintainer="mingcheng <mingcheng@apache.org>"

# Replace Debian apt sources with Tsinghua mirrors
RUN sed -i 's|deb.debian.org|mirrors.tuna.tsinghua.edu.cn|g' /etc/apt/sources.list.d/debian.sources && \
    sed -i 's|security.debian.org|mirrors.tuna.tsinghua.edu.cn|g' /etc/apt/sources.list.d/debian.sources

# Configure Rustup to use Tsinghua mirrors
ENV RUSTUP_DIST_SERVER=https://mirrors.tuna.tsinghua.edu.cn/rustup \
    RUSTUP_UPDATE_ROOT=https://mirrors.tuna.tsinghua.edu.cn/rustup/rustup

# Configure Cargo to use Tsinghua crates.io mirror
RUN mkdir -p /usr/local/cargo && \
    echo '[source.crates-io]' > /usr/local/cargo/config.toml && \
    echo 'replace-with = "tuna"' >> /usr/local/cargo/config.toml && \
    echo '' >> /usr/local/cargo/config.toml && \
    echo '[source.tuna]' >> /usr/local/cargo/config.toml && \
    echo 'registry = "sparse+https://mirrors.tuna.tsinghua.edu.cn/crates.io-index/"' >> /usr/local/cargo/config.toml

# Install build dependencies required for compilation
RUN apt update && apt install -y --no-install-recommends \
    build-essential \
    git \
    libssl-dev \
    pkg-config \
    perl \
    && rm -rf /var/lib/apt/lists/*

# Ensure we're using the latest stable Rust toolchain
RUN rustup default stable && rustup update stable

# Set the working directory for dependency building
WORKDIR /build

# Copy only dependency manifests first to leverage Docker layer caching
# Note: Not copying Cargo.lock here to allow cargo to resolve versions from mirror
COPY Cargo.toml /build/

# Create a dummy source file to build dependencies
RUN mkdir src && \
    echo "fn main() {}" > src/main.rs && \
    cargo build --release && \
    rm -rf src target/release/deps/mail_forwarder*

# Stage 2: Builder - Build the actual application
FROM dependencies AS builder

# Copy the actual source code
COPY . .

# Build the application with optimizations
RUN cargo build --release && \
    strip target/release/mail-forwarder && \
    cp target/release/mail-forwarder /bin/mail-forwarder

# Stage 3: Runtime - Create minimal runtime image
FROM debian:trixie AS runtime

# Replace Debian apt sources with Tsinghua mirrors
RUN sed -i 's|deb.debian.org|mirrors.tuna.tsinghua.edu.cn|g' /etc/apt/sources.list.d/debian.sources && \
    sed -i 's|security.debian.org|mirrors.tuna.tsinghua.edu.cn|g' /etc/apt/sources.list.d/debian.sources

# Install only runtime dependencies
RUN apt update && apt install -y --no-install-recommends \
    tzdata \
    curl \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Set working directory
WORKDIR /app

# Copy the compiled binary from builder stage
COPY --from=builder /bin/mail-forwarder /bin/mail-forwarder

# Define the entrypoint
ENTRYPOINT ["/bin/mail-forwarder"]

# Default command (can be overridden)
CMD ["--config", "/app/config.toml"]
