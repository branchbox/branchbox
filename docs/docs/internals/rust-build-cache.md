---
sidebar_position: 7
---

# Rust build caching

The mbx experiment is withdrawn. BranchBox development and manual CLI CI use plain Cargo. The measurements below retain the experiment's results and limits; they do not recommend installing mbx or adopting it globally.

## Current CI policy

Main CI runs one matrix for Rust, generic, Rails and Node, builds with `cargo build -p branchbox-cli`, and invokes the manual harness directly. [`Swatinem/rust-cache@v2`](https://github.com/Swatinem/rust-cache#readme), already used elsewhere in CI, caches Cargo dependencies with `shared-key: manual-cli-e2e`. Only the Rust stack on a main-branch push may save through `save-if`; the other stacks restore. The standalone schedule/dispatch workflow sets `save-if: false` for all four stacks. No mbx step or Cargo wrapper remains. The harness still finds `target/debug/branchbox`.

No performance improvement has yet been measured for this replacement. Coverage and release jobs retain their existing cache configuration.

## Historical local benchmark

The October 4, 2026 benchmark builds `branchbox-cli` from commit `006dab5151a75b0bed89c6ebdb5e73ddab71a4c2` on an Apple Silicon Mac with 12 logical CPUs and 24 GiB RAM. It uses Cargo/Rust 1.90.0, macOS SDK 26.2, default debug features, the system linker and two build jobs.

| Scenario | Cargo median (range) | mbx median (range) | mbx change |
| --- | ---: | ---: | ---: |
| Cold build | 96.21s (95.66–97.80) | 121.28s (121.04–124.86) | +26.1% |
| Second checkout, empty target | 95.86s (95.82–96.43) | 96.33s (96.10–97.84) | +0.5% |
| First core edit | 2.38s (2.30–2.72) | 7.46s (7.40–7.51) | +213.4% |
| Later core edit | 1.81s (1.73–1.82) | 2.36s (2.26–2.48) | +30.4% |

Positive changes mean slower builds. Across these trials, local checkout reuse provided no meaningful wall-time reduction, while cold builds and edits were slower. These results did not justify adoption. [Download the 24-build raw results](@site/static/benchmarks/rust-cache-2026-10-04.json).


Three independent trials use fresh compiler outputs and mbx stores, with dependencies fetched before timing. Builds run sequentially, alternating tool order. The second source snapshot gets a different empty target directory; only mbx retains its compilation store. Cargo keeps its normal incremental behavior; mbx uses its default learned incremental behavior.

Two temporary edits change the naming limit from three to four, then five words. After every build, the resulting CLI must generate the expected slug, accept a valid name, reject an invalid name and print help. These edits never enter application source. Timings come from macOS `/usr/bin/time -l`, excluding monitoring delay. Maximum RSS does not measure aggregate machine memory.

The first mbx reuse build recorded 409 hits and 1,134 unconsulted compilations. Its OpenSSL and libgit2 build scripts took 72.28 and 11.14 seconds. Native build work still dominates this configuration; the precise cache-key reason was not established. Overlapping compiler durations and mbx's estimated time avoided are not wall-clock savings.

### Historical reproduction on macOS

Reproducing this withdrawn experiment requires a separately verified [Mr. Boxington 1.22.0](https://github.com/jdx/mr-boxington/releases/tag/v1.22.0) binary. The archive `mbx-aarch64-apple-darwin.tar.gz` used here has SHA-256 `e548b5758498cf822a180b6328597e6aded8fe9bb3046cd918399172ae30dde2`. Upstream documents [archive verification](https://mr-boxington.jdx.dev/installation). This is historical reproduction guidance, not a BranchBox installation recommendation.

Run from the BranchBox repository with the verified binary and a new output directory:

```bash
python3 scripts/benchmark-rust-cache.py \
  --ref 006dab5151a75b0bed89c6ebdb5e73ddab71a4c2 \
  --mbx /path/to/verified/mbx \
  --output /private/tmp/branchbox-cache-new-trial \
  --trials 3 --jobs 2
```

The helper isolates Cargo downloads, targets and mbx stores, rejects existing mbx configuration, checks remote caching is disabled, reserves disk space and removes only generated trial directories. Logs and JSON results remain. It does not install a Cargo shim or configure the developer's machine. Python 3.9 or newer is supported.

## Historical CI pilot

The withdrawn jobs pinned action commit `d0825fbaf3cc36ca2609aa38e71046265a1f1e37` and mbx 1.22.0. Their GitHub `target` payload restored Cargo's target tree and downloads, unlike the local benchmark's object-store reuse. They used `mbx build` and a Bash-exported function that routed repeated harness `cargo build` calls through mbx.

The [first main run](https://github.com/branchbox/branchbox/actions/runs/37295055095), at merge commit `dd08fd7c795943577172bf1a940b8e03b576767d`, passed its harnesses but published archives from all four stacks, violating the intended Rust-only writer policy. Writing `ACTIONS_CACHE_MODE=read` through `GITHUB_ENV` did not restrict the runner's cache permissions. A follow-up repair used [job-level `cache-mode: read`](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax#jobsjob_idcache-mode) and reader eligibility assertions. The experiment has since been withdrawn in favor of the current Cargo policy above.

### Observed cold and warm CI pair

The mbx summary excludes Cargo's fresh-output reuse and the action's restore/save overhead, as the [action documentation](https://mr-boxington.jdx.dev/github-action) explains. Complete-job measurements are needed to assess build changes.

The [uncached baseline run](https://github.com/branchbox/branchbox/actions/runs/37248005821) at the benchmark commit spent 129–132 seconds in each stack's `Build CLI` step, totaling 521 runner seconds across the four jobs. This excludes setup and harness execution.

One same-head PR run provides a separate comparison between the mbx pilot's [cold attempt](https://github.com/branchbox/branchbox/actions/runs/37251057735/attempts/1) and [warm attempt](https://github.com/branchbox/branchbox/actions/runs/37251057735/attempts/2). All four harnesses passed both attempts; the warm jobs restored the cold Rust job's cache.

| Four-worker aggregate | Cold mbx attempt | Warm mbx attempt | Change |
| --- | ---: | ---: | ---: |
| `Build CLI` steps | 496s | 55s | −88.9% |
| Complete manual CLI jobs | 1,586s | 1,192s | −24.8% |

These totals sum four parallel workers, not whole-workflow elapsed time. The complete-job total includes setup, restore, build, harness and save work; setup/restore plus mbx post-step time increased from 15 to 37 seconds in this pair. One pair does not establish repeatable savings, and runner, network and Docker work can vary. The pilot added caching to previously uncached harness jobs; it did not demonstrate mbx superiority over a plain Cargo target cache. Its cold/warm observations and local results do not justify global adoption or a performance claim for the replacement.
