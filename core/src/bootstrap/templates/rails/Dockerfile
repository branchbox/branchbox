ARG RUBY_VERSION="3.3"
FROM mcr.microsoft.com/devcontainers/ruby:${RUBY_VERSION}

USER root

# Install Rails dependencies
RUN apt-get update && apt-get install -y \
    build-essential \
    git \
    curl \
    libpq-dev \
    nodejs \
    npm \
    && rm -rf /var/lib/apt/lists/*

# Install Rails
RUN gem install rails bundler

USER vscode
