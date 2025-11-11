#!/usr/bin/env bash
# Regenerate SwiftProtobuf + gRPC Swift stubs for the macOS app.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="macos/Sources/BranchBoxApp/Generated"
CACHE_ROOT="${REPO_ROOT}/.build/swift-proto-tools"
SWIFT_PROTOBUF_REF="${SWIFT_PROTOBUF_REF:-1.33.3}"
GRPC_SWIFT_REF="${GRPC_SWIFT_REF:-1.27.0}"

mkdir -p "${REPO_ROOT}/${OUT_DIR}"
mkdir -p "${CACHE_ROOT}"

docker run --rm \
  -v "${REPO_ROOT}":/work \
  -v "${CACHE_ROOT}":/cache \
  -w /work \
  swift:6.1 /bin/bash -lc "
set -euo pipefail
apt-get update >/dev/null
apt-get install -y git protobuf-compiler >/dev/null

if [ ! -d /cache/swift-protobuf ]; then
  git clone --depth 1 --branch ${SWIFT_PROTOBUF_REF} --recurse-submodules https://github.com/apple/swift-protobuf.git /cache/swift-protobuf >/dev/null
else
  cd /cache/swift-protobuf
  git fetch origin ${SWIFT_PROTOBUF_REF} --depth 1 >/dev/null
  git checkout -f ${SWIFT_PROTOBUF_REF} >/dev/null
fi

cd /cache/swift-protobuf
swift build -c release >/dev/null
cp .build/release/protoc-gen-swift /usr/local/bin/

if [ ! -d /cache/grpc-swift ]; then
  git clone --depth 1 --branch ${GRPC_SWIFT_REF} --recurse-submodules https://github.com/grpc/grpc-swift.git /cache/grpc-swift >/dev/null
else
  cd /cache/grpc-swift
  git fetch origin ${GRPC_SWIFT_REF} --depth 1 >/dev/null
  git checkout -f ${GRPC_SWIFT_REF} >/dev/null
fi

cd /cache/grpc-swift
swift build -c release --product protoc-gen-grpc-swift >/dev/null
cp .build/release/protoc-gen-grpc-swift /usr/local/bin/

cd /work
protoc \\
  --swift_out=${OUT_DIR} \\
  --grpc-swift_out=Client=true,Server=false:${OUT_DIR} \\
  --proto_path=agent/proto \\
  agent/proto/agent.proto
"

echo "✔ Generated Swift stubs in ${OUT_DIR}"
