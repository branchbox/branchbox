# Rust development environment
FROM mcr.microsoft.com/devcontainers/rust:latest

# Switch to root for system dependencies
USER root

# Install additional tools for worktree development
RUN apt-get update && export DEBIAN_FRONTEND=noninteractive \
    && apt-get -y install --no-install-recommends \
    git \
    curl \
    ca-certificates \
    gnupg \
    build-essential \
    pkg-config \
    libssl-dev \
    protobuf-compiler \
    clang \
    libsqlite3-dev \
    libncurses5-dev \
    zlib1g-dev \
    libcurl4-openssl-dev \
    libicu-dev \
    libxml2-dev \
    libedit-dev \
    libpython3-dev \
    && rm -rf /var/lib/apt/lists/*

# Install Swift toolchain (used for the macOS app workspace)
ARG SWIFT_VERSION=5.10.1
ARG SWIFT_PLATFORM=swift-${SWIFT_VERSION}-RELEASE-debian12
RUN curl -fsSL https://download.swift.org/swift-${SWIFT_VERSION}-release/debian12/${SWIFT_PLATFORM}.tar.gz -o /tmp/swift.tar.gz \
    && mkdir -p /usr/local/swift \
    && tar -xzf /tmp/swift.tar.gz --directory /usr/local/swift --strip-components=1 \
    && rm /tmp/swift.tar.gz

# Install useful Rust tools as root, then fix permissions
ARG MDBOOK_VERSION=0.4.40
RUN cargo install cargo-watch cargo-edit cargo-expand \
    && cargo install mdbook --locked --version ${MDBOOK_VERSION} \
    && cargo install cargo-release --locked \
    && chown -R vscode:rustlang /usr/local/cargo \
    && chmod -R g+w /usr/local/cargo

# Switch back to vscode user
USER vscode

# Set up Rust environment
ENV CARGO_HOME=/usr/local/cargo \
    RUSTUP_HOME=/usr/local/rustup \
    PATH=/usr/local/swift/usr/bin:/usr/local/cargo/bin:$PATH
