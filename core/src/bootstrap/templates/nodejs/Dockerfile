FROM mcr.microsoft.com/devcontainers/javascript-node:20

USER root

RUN apt-get update && apt-get install -y \
    git \
    curl \
    && rm -rf /var/lib/apt/lists/*

USER node
