# BranchBox Node.js Devcontainer
# Supports any Node version via mise version manager

FROM mcr.microsoft.com/devcontainers/base:debian

USER root

# Install build dependencies for native Node modules
RUN apt-get update && apt-get install -y \
    build-essential \
    git \
    curl \
    # Python is required for node-gyp (native module compilation)
    python3 \
    python3-pip \
    # Common native module dependencies
    libpq-dev \
    libsqlite3-dev \
    # Image processing (sharp, jimp, etc.)
    libvips-dev \
    # Canvas support
    libcairo2-dev \
    libpango1.0-dev \
    libjpeg-dev \
    libgif-dev \
    librsvg2-dev \
    && rm -rf /var/lib/apt/lists/*

# Install mise for Node version management (reads .nvmrc, .node-version, .tool-versions)
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

# Pre-install common Node versions to speed up first run
# Projects can override via .nvmrc, .node-version, or .tool-versions
RUN /home/vscode/.local/bin/mise install node@20 node@22 && \
    /home/vscode/.local/bin/mise use --global node@20

ENV PATH="/home/vscode/.local/bin:/home/vscode/.local/share/mise/shims:$PATH"
