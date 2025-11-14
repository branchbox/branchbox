# Rust-focused development environment
FROM mcr.microsoft.com/devcontainers/rust:latest

# Switch to root for system dependencies
USER root

ARG MDBOOK_VERSION=0.4.40

# Install additional tools for worktree development
RUN apt-get update && export DEBIAN_FRONTEND=noninteractive \
    && apt-get -y install --no-install-recommends \
    git \
    curl \
    ca-certificates \
    gnupg \
    unzip \
    tzdata \
    build-essential \
    pkg-config \
    libssl-dev \
    protobuf-compiler \
    clang \
    libsqlite3-dev \
    libsqlite3-0 \
    libncurses5-dev \
    libcurl4-openssl-dev \
    libicu-dev \
    libxml2-dev \
    libbsd-dev \
    libedit-dev \
    libedit2 \
    libz3-dev \
    zlib1g-dev \
    libzip-dev \
    libatomic1 \
    libpython3-dev \
    && rm -rf /var/lib/apt/lists/*

# Install useful Rust tools as root, then fix permissions
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
    PATH=/usr/local/cargo/bin:$PATH
