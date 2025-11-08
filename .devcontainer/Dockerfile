# Rust development environment
FROM mcr.microsoft.com/devcontainers/rust:latest

# Switch to root for system dependencies
USER root

# Install additional tools for worktree development
# clang and lld provide faster linking
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
    lld \
    && rm -rf /var/lib/apt/lists/*

# Install useful Rust tools and sccache as root, then fix permissions
ARG MDBOOK_VERSION=0.4.40
RUN cargo install cargo-watch cargo-edit cargo-expand sccache \
    && cargo install mdbook --locked --version ${MDBOOK_VERSION} \
    && cargo install cargo-release --locked \
    && chown -R vscode:rustlang /usr/local/cargo \
    && chmod -R g+w /usr/local/cargo

# Switch back to vscode user
USER vscode

# Set up Rust environment with sccache
ENV CARGO_HOME=/usr/local/cargo \
    RUSTUP_HOME=/usr/local/rustup \
    PATH=/usr/local/cargo/bin:$PATH \
    RUSTC_WRAPPER=/usr/local/cargo/bin/sccache \
    SCCACHE_CACHE_SIZE=15G
