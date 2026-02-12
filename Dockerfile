# Stage 1: Dependencies - Cache Rust dependencies separately for faster rebuilds
FROM rust:alpine AS dependencies
LABEL maintainer="mingcheng <mingcheng@apache.org>"

# Install build dependencies required for compilation
RUN apk add --no-cache \
    build-base \
    git \
    musl-dev \
    libressl-dev \
    pkgconfig \
    perl

# Ensure we're using the latest stable Rust toolchain
RUN rustup default stable && rustup update stable

# Set the working directory for dependency building
WORKDIR /build

# Copy only dependency manifests first to leverage Docker layer caching
COPY Cargo.toml Cargo.lock /build/

# Create a dummy source file to build dependencies
RUN mkdir src && \
    echo "fn main() {}" > src/main.rs && \
    cargo build --release && \
    rm -rf src

# Stage 2: Builder - Build the actual application
FROM dependencies AS builder

# Copy the actual source code
COPY . .

# Build the application with optimizations
RUN cargo build --release && \
    strip target/release/mail-forwarder && \
    cp target/release/mail-forwarder /bin/mail-forwarder

# Stage 3: Runtime - Create minimal runtime image
FROM alpine AS runtime

# Set timezone (configurable via build args)
ARG TZ=Asia/Shanghai
ENV TZ=${TZ}

# Install only runtime dependencies
RUN apk add --no-cache \
    tzdata \
    git \
    curl \
    ca-certificates && \
    ln -snf /usr/share/zoneinfo/$TZ /etc/localtime && \
    echo $TZ > /etc/timezone && \
    # Clean up apk cache to reduce image size
    rm -rf /var/cache/apk/*

# Copy the compiled binary from builder stage
COPY --from=builder /bin/mail-forwarder /bin/mail-forwarder

# Create a non-root user for security
RUN addgroup -g 1000 mailer && \
    adduser -D -u 1000 -G mailer mailer

# Set the working directory
WORKDIR /repo

# Change ownership of the working directory
RUN chown -R mailer:mailer /repo

# Switch to non-root user
USER mailer

# Define the entrypoint
ENTRYPOINT ["/bin/mail-forwarder"]

# Default command (can be overridden)
CMD ["--config", "/repo/config.toml"]
