---
sidebar_position: 7
---

# Rust build cache pilot

BranchBox is evaluating [Mr. Boxington 1.22.0](https://github.com/jdx/mr-boxington/releases/tag/v1.22.0) in the four manual CLI harness stacks of the main and scheduled CI workflows. Other Rust jobs retain their existing cache configuration.

## Local benchmark

The October 4, 2026 benchmark builds `branchbox-cli` from commit `006dab5151a75b0bed89c6ebdb5e73ddab71a4c2` on an Apple Silicon Mac with 12 logical CPUs and 24 GiB RAM. It uses Cargo/Rust 1.90.0, macOS SDK 26.2, default debug features, the system linker and two build jobs.

| Scenario | Cargo median (range) | mbx median (range) | mbx change |
| --- | ---: | ---: | ---: |
| Cold build | 96.21s (95.66–97.80) | 121.28s (121.04–124.86) | +26.1% |
| Second checkout, empty target | 95.86s (95.82–96.43) | 96.33s (96.10–97.84) | +0.5% |
| First core edit | 2.38s (2.30–2.72) | 7.46s (7.40–7.51) | +213.4% |
| Later core edit | 1.81s (1.73–1.82) | 2.36s (2.26–2.48) | +30.4% |

Positive changes mean slower builds. Across these trials, local checkout reuse provided no meaningful wall-time reduction, while cold builds and edits were slower. Keep local adoption optional pending a configuration with measured improvement. [Download raw results](@site/static/benchmarks/rust-cache-2026-10-04.json).


Three independent trials use fresh compiler outputs and mbx stores, with dependencies fetched before timing. Builds run sequentially, alternating tool order. The second source snapshot gets a different empty target directory; only mbx retains its compilation store. Cargo keeps its normal incremental behavior; mbx uses its default learned incremental behavior.

Two temporary edits change the naming limit from three to four, then five words. After every build, the resulting CLI must generate the expected slug, accept a valid name, reject an invalid name and print help. These edits never enter application source. Timings come from macOS `/usr/bin/time -l`, excluding monitoring delay. Maximum RSS does not measure aggregate machine memory.

The first mbx reuse build recorded 409 hits and 1,134 unconsulted compilations. Its OpenSSL and libgit2 build scripts took 72.28 and 11.14 seconds. Native build work still dominates this configuration; the precise cache-key reason was not established. Overlapping compiler durations and mbx's estimated time avoided are not wall-clock savings.

### Reproduce on macOS

Use the official prebuilt binary, preserving the current Rust toolchain. The release 1.22.0 archive `mbx-aarch64-apple-darwin.tar.gz` used here has SHA-256 `e548b5758498cf822a180b6328597e6aded8fe9bb3046cd918399172ae30dde2`. Follow the [official installation instructions](https://mr-boxington.jdx.dev/installation) to download and verify it.

Run from the BranchBox repository with the verified binary and a new output directory:

```bash
python3 scripts/benchmark-rust-cache.py \
  --ref 006dab5151a75b0bed89c6ebdb5e73ddab71a4c2 \
  --mbx /path/to/verified/mbx \
  --output /private/tmp/branchbox-cache-new-trial \
  --trials 3 --jobs 2
```

The helper isolates Cargo downloads, targets and mbx stores, rejects existing mbx configuration, checks remote caching is disabled, reserves disk space and removes only generated trial directories. Logs and JSON results remain. It does not install a Cargo shim or configure the developer's machine. Python 3.9 or newer is supported.

## CI pilot

The jobs pin action commit `d0825fbaf3cc36ca2609aa38e71046265a1f1e37` and mbx 1.22.0. Their GitHub `target` payload restores Cargo's target tree and downloads, unlike the local benchmark's object-store reuse. The harness still finds `target/debug/branchbox`.

The explicit build uses `mbx build`. A function exported only for the harness step routes its current repeated `cargo build` through mbx. Review that wrapper if future harness changes invoke other Cargo commands or run Cargo outside Bash.

Main CI separates the Rust writer from a generic/Rails/Node reader matrix. Readers restore compatible entries through shared prefixes and have literal job-level `cache-mode: read`. GitHub [enforces this permission through scoped cache tokens](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax#jobsjob_idcache-mode), allowing restores while denying saves. The action's `github-cache-mode: target` selects the payload format; it does not set that permission.

The Rust job leaves `cache-mode` unset to retain GitHub's event and fork defaults. The pinned action permits saves after successful default-branch pushes and opts Rust into same-repository pull request scoped saves for warm pilot validation; forks cannot save. All four schedule and workflow-dispatch stacks have literal `cache-mode: read`. Reader jobs require the action's `cache-save-eligible` output to be exactly `false` before building. A true or missing output fails the job, also preventing the action's success-gated save. See the [pinned action policy](https://github.com/jdx/mr-boxington-action/blob/d0825fbaf3cc36ca2609aa38e71046265a1f1e37/src/lib.ts).

### Initial rollout and policy repair

The [first main run](https://github.com/branchbox/branchbox/actions/runs/37295055095), at merge commit `dd08fd7c795943577172bf1a940b8e03b576767d`, passed its harnesses but published archives from all four stacks. This violated the intended Rust-only writer policy. The initial workflow tried to restrict readers by writing `ACTIONS_CACHE_MODE=read` through `GITHUB_ENV`; the runner supplied the effective mode from the job's cache permission instead. The repair moves that restriction to literal job-level `cache-mode: read` and adds the eligibility assertion above. Validate the repaired main run's save/skip logs and cache keys before treating the writer policy as confirmed in production.

### Observed cold and warm CI pair

Compare the complete job's install, restore, build, harness and save times before widening the pilot. The mbx summary excludes Cargo's fresh-output reuse and the action's restore/save overhead, as the [action documentation](https://mr-boxington.jdx.dev/github-action) explains.

The [uncached baseline run](https://github.com/branchbox/branchbox/actions/runs/37248005821) at the benchmark commit spent 129–132 seconds in each stack's `Build CLI` step, totaling 521 runner seconds across the four jobs. This excludes setup and harness execution.

One same-head PR run provides a separate comparison between the mbx pilot's [cold attempt](https://github.com/branchbox/branchbox/actions/runs/37251057735/attempts/1) and [warm attempt](https://github.com/branchbox/branchbox/actions/runs/37251057735/attempts/2). All four harnesses passed both attempts; the warm jobs restored the cold Rust job's cache.

| Four-worker aggregate | Cold mbx attempt | Warm mbx attempt | Change |
| --- | ---: | ---: | ---: |
| `Build CLI` steps | 496s | 55s | −88.9% |
| Complete manual CLI jobs | 1,586s | 1,192s | −24.8% |

These totals sum four parallel workers, not whole-workflow elapsed time. The complete-job total includes setup, restore, build, harness and save work; setup/restore plus mbx post-step time increased from 15 to 37 seconds in this pair. One pair does not establish repeatable savings, and runner, network and Docker work can vary. This pilot adds caching to previously uncached harness jobs; it does not demonstrate mbx superiority over a plain Cargo target cache. Local results and mbx's compilation estimates alone do not establish CI savings.

Coverage retains its existing setup because `cargo-llvm-cov` uses a compiler wrapper. Release jobs retain their cache policy. No remote bucket or cache service is introduced.
