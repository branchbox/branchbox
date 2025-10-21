FROM mcr.microsoft.com/devcontainers/base:ubuntu

USER root

RUN apt-get update && apt-get install -y \
    build-essential \
    git \
    curl \
    vim \
    && rm -rf /var/lib/apt/lists/*

USER vscode
