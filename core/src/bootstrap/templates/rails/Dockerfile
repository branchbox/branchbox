ARG RUBY_VERSION="3.3"
FROM ghcr.io/rails/devcontainer/images/ruby:${RUBY_VERSION}

USER root

RUN apt-get update && apt-get install -y \
    build-essential \
    git \
    curl \
    && rm -rf /var/lib/apt/lists/*

USER vscode
