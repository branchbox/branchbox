# BranchBox Remotion Demo

This package renders a short BranchBox teaser video from a deterministic composition.
The terminal scenes are based on real CLI output captured from `scripts/demo-teaser.sh`.

## Quick start

From the repository root:

```bash
./scripts/remotion-demo.sh --stack rust
```

Output defaults to:

```text
demos/remotion/out/branchbox-teaser-rust.mp4
```

Final 4-stack batch render (recommended for release assets):

```bash
./scripts/remotion-demo.sh --all-stacks
```

This writes:

```text
demos/remotion/out/branchbox-teaser-rust-final.mp4
demos/remotion/out/branchbox-teaser-node-final.mp4
demos/remotion/out/branchbox-teaser-rails-final.mp4
demos/remotion/out/branchbox-teaser-generic-final.mp4
```

Publish docs/website assets and per-doc cuts:

```bash
./scripts/remotion-docs-all.sh --target docs
# or:
./scripts/remotion-docs-all.sh --target both
```

## Run in Studio

```bash
./scripts/remotion-demo.sh --studio --stack node
```

## Devcontainer Linux dependencies

If browser dependencies are missing inside Linux/devcontainers, install once with:

```bash
./scripts/remotion-demo.sh --install-linux-deps --stack rust
```

## Notes

- The script runs `npm install` automatically on first run.
- Set `REMOTION_CHROME_MODE=chrome-for-testing` to prefer a full Chrome binary.
- Current pacing target is ~51 seconds per render.
- Refresh source output samples by running:
  - `scripts/demo-teaser.sh --stack rust > /tmp/branchbox-demo-rust.log 2>&1`
  - `scripts/demo-teaser.sh --stack node > /tmp/branchbox-demo-node.log 2>&1`
  - `scripts/demo-teaser.sh --stack rails > /tmp/branchbox-demo-rails.log 2>&1`
  - `scripts/demo-teaser.sh --stack generic > /tmp/branchbox-demo-generic.log 2>&1`
