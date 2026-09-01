FROM ubuntu:24.04 AS build

ARG LINUX_VERSION=6.1.155
ARG FIRECRACKER_VERSION=v1.16.1
ARG DEBIAN_FRONTEND=noninteractive

RUN apt-get update \
    && apt-get install -y --no-install-recommends bc bison build-essential ca-certificates curl flex libelf-dev libssl-dev xz-utils \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /src
RUN curl --http1.1 --retry 5 --retry-delay 2 --retry-all-errors -fsSLo linux.tar.xz \
      "https://cdn.kernel.org/pub/linux/kernel/v6.x/linux-${LINUX_VERSION}.tar.xz" \
    && curl --http1.1 --retry 5 --retry-delay 2 --retry-all-errors -fsSLo firecracker.tar.gz \
      "https://github.com/firecracker-microvm/firecracker/archive/refs/tags/${FIRECRACKER_VERSION}.tar.gz" \
    && tar -xf linux.tar.xz \
    && tar -xf firecracker.tar.gz

COPY kernel-required.config /kernel-required.config

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
    && scripts/config --enable CONFIG_OVERLAY_FS \
    && scripts/config --enable CONFIG_EXT4_FS \
    && while IFS= read -r requirement; do scripts/config --enable "${requirement%=y}"; done < /kernel-required.config \
    && make olddefconfig \
    && while IFS= read -r requirement; do grep -Fqx "$requirement" .config; done < /kernel-required.config \
    && make -j"$(nproc)" vmlinux

FROM scratch AS export
COPY --from=build /src/linux-6.1.155/vmlinux /vmlinux
COPY --from=build /src/linux-6.1.155/.config /kernel.config
