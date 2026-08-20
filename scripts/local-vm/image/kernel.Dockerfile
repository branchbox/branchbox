FROM ubuntu:24.04 AS build

ARG LINUX_VERSION=6.1.155
ARG FIRECRACKER_VERSION=v1.16.1
ARG DEBIAN_FRONTEND=noninteractive

RUN apt-get update \
    && apt-get install -y --no-install-recommends bc bison build-essential ca-certificates curl flex libelf-dev libssl-dev xz-utils \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /src
RUN curl -fsSLo linux.tar.xz "https://cdn.kernel.org/pub/linux/kernel/v6.x/linux-${LINUX_VERSION}.tar.xz" \
    && curl -fsSLo firecracker.tar.gz "https://github.com/firecracker-microvm/firecracker/archive/refs/tags/${FIRECRACKER_VERSION}.tar.gz" \
    && tar -xf linux.tar.xz \
    && tar -xf firecracker.tar.gz

WORKDIR /src/linux-${LINUX_VERSION}
RUN cp "/src/firecracker-${FIRECRACKER_VERSION#v}/resources/guest_configs/microvm-kernel-ci-x86_64-6.1.config" .config \
    && scripts/config --enable CONFIG_CGROUPS \
    && scripts/config --enable CONFIG_CGROUP_BPF \
    && scripts/config --enable CONFIG_CGROUP_CPUACCT \
    && scripts/config --enable CONFIG_CGROUP_DEVICE \
    && scripts/config --enable CONFIG_CGROUP_FREEZER \
    && scripts/config --enable CONFIG_CGROUP_PIDS \
    && scripts/config --enable CONFIG_CGROUP_SCHED \
    && scripts/config --enable CONFIG_CPUSETS \
    && scripts/config --enable CONFIG_IP_NF_FILTER \
    && scripts/config --enable CONFIG_IP_NF_NAT \
    && scripts/config --enable CONFIG_NETFILTER \
    && scripts/config --enable CONFIG_NETFILTER_XT_MATCH_ADDRTYPE \
    && scripts/config --enable CONFIG_NETFILTER_XT_MATCH_CONNTRACK \
    && scripts/config --enable CONFIG_NETFILTER_XT_MATCH_MULTIPORT \
    && scripts/config --enable CONFIG_NETFILTER_XT_MATCH_STATE \
    && scripts/config --enable CONFIG_NETFILTER_XT_TARGET_MASQUERADE \
    && scripts/config --enable CONFIG_NET_NS \
    && scripts/config --enable CONFIG_BRIDGE \
    && scripts/config --enable CONFIG_BRIDGE_NETFILTER \
    && scripts/config --enable CONFIG_VETH \
    && scripts/config --enable CONFIG_OVERLAY_FS \
    && scripts/config --enable CONFIG_EXT4_FS \
    && make olddefconfig \
    && make -j"$(nproc)" vmlinux

FROM scratch AS export
COPY --from=build /src/linux-6.1.155/vmlinux /vmlinux
