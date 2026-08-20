FROM ubuntu:24.04

ARG DEBIAN_FRONTEND=noninteractive
ARG DEVCONTAINER_CLI_VERSION=0.80.3

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
       ca-certificates curl docker.io git iproute2 iptables jq nodejs npm openssh-server \
       rsync socat sudo systemd systemd-sysv tini \
    && npm install --global "@devcontainers/cli@${DEVCONTAINER_CLI_VERSION}" \
    && npm cache clean --force \
    && useradd --create-home --shell /bin/bash --uid 1000 branchbox \
    && usermod -aG docker branchbox \
    && printf 'branchbox ALL=(ALL) NOPASSWD:ALL\n' >/etc/sudoers.d/branchbox \
    && chmod 0440 /etc/sudoers.d/branchbox \
    && install -d -m 0700 -o branchbox -g branchbox /home/branchbox/.ssh \
    && install -d /run/sshd /workspaces \
    && printf 'PasswordAuthentication no\nPermitRootLogin no\nAllowUsers branchbox\n' >/etc/ssh/sshd_config.d/branchbox.conf \
    && systemctl enable docker.service ssh.service systemd-networkd.service systemd-resolved.service \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/* /var/cache/apt/* /etc/machine-id \
    && touch /etc/machine-id

COPY 20-eth0.network /etc/systemd/network/20-eth0.network
COPY branchbox-firstboot.service /etc/systemd/system/branchbox-firstboot.service

RUN systemctl enable branchbox-firstboot.service \
    && ln -sf /run/systemd/resolve/stub-resolv.conf /etc/resolv.conf

STOPSIGNAL SIGRTMIN+3
CMD ["/sbin/init"]
