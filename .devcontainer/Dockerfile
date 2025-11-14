# Rust-focused development environment
FROM mcr.microsoft.com/devcontainers/rust:latest

# Switch to root for system dependencies
USER root

ARG MDBOOK_VERSION=0.4.40

# Install additional tools for worktree development
RUN apt-get update && export DEBIAN_FRONTEND=noninteractive \
    && apt-get -y install --no-install-recommends \
    build-essential \
    ca-certificates \
    clang \
    curl \
    git \
    gnupg \
    libatomic1 \
    libbsd-dev \
    libcurl4-openssl-dev \
    libedit-dev \
    libedit2 \
    libicu-dev \
    libncurses5-dev \
    libpython3-dev \
    libsqlite3-0 \
    libsqlite3-dev \
    libssl-dev \
    libxml2-dev \
    libz3-dev \
    libzip-dev \
    pkg-config \
    protobuf-compiler \
    tzdata \
    unzip \
    zlib1g-dev \
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
