# BranchBox Rails Devcontainer
# Supports any Ruby version via mise version manager

FROM mcr.microsoft.com/devcontainers/base:debian

USER root

# Install build dependencies for Ruby compilation and common Rails gems
RUN apt-get update && apt-get install -y \
    build-essential \
    git \
    curl \
    libssl-dev \
    libreadline-dev \
    zlib1g-dev \
    libyaml-dev \
    libffi-dev \
    libgmp-dev \
    # Database clients (for apps using Postgres/MySQL)
    libpq-dev \
    default-libmysqlclient-dev \
    # SQLite support
    sqlite3 \
    libsqlite3-dev \
    # Image processing (ActiveStorage, image manipulation)
    imagemagick \
    libvips \
    libopenslide0 \
    # PDF processing
    poppler-utils \
    && rm -rf /var/lib/apt/lists/*

# Install gum (for prettier bin/setup output in modern Rails apps)
RUN mkdir -p /etc/apt/keyrings && \
    curl -fsSL https://repo.charm.sh/apt/gpg.key | gpg --dearmor -o /etc/apt/keyrings/charm.gpg && \
    echo "deb [signed-by=/etc/apt/keyrings/charm.gpg] https://repo.charm.sh/apt/ * *" | tee /etc/apt/sources.list.d/charm.list && \
    apt-get update && apt-get install -y gum && \
    rm -rf /var/lib/apt/lists/*

# Install mise for Ruby version management (reads .ruby-version, .mise.toml, .tool-versions)
RUN curl https://mise.run | sh && \
    /root/.local/bin/mise settings set experimental true && \
    /root/.local/bin/mise settings set legacy_version_file true && \
    echo 'eval "$(/root/.local/bin/mise activate bash)"' >> /etc/bash.bashrc

USER vscode

# Install mise for vscode user
RUN curl https://mise.run | sh && \
    /home/vscode/.local/bin/mise settings set experimental true && \
    /home/vscode/.local/bin/mise settings set legacy_version_file true && \
    echo 'eval "$(/home/vscode/.local/bin/mise activate bash)"' >> ~/.bashrc && \
    echo 'eval "$(/home/vscode/.local/bin/mise activate zsh)"' >> ~/.zshrc

# Pre-install common Ruby versions to speed up first run
# Projects can override via .ruby-version or .mise.toml
RUN /home/vscode/.local/bin/mise install ruby@3.3 ruby@3.4 && \
    /home/vscode/.local/bin/mise use --global ruby@3.3

ENV PATH="/home/vscode/.local/bin:/home/vscode/.local/share/mise/shims:$PATH"
