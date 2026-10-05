#!/usr/bin/env python3
"""Compare plain Cargo and explicit mbx builds of BranchBox's CLI on macOS.

Uses archived source, separate targets, an isolated Cargo registry, and a fresh
mbx cache per trial. Does not install tools, configure Cargo, or change the repo.
"""

import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import signal
import stat
import subprocess
import sys
import time


GIB = 1024**3
TIMING = re.compile(r"^\s*([\d.]+) real\s+([\d.]+) user\s+([\d.]+) sys\s*$", re.M)
MAX_RSS = re.compile(r"^\s*(\d+)\s+maximum resident set size\s*$", re.M)


def executable(value):
    found = shutil.which(value)
    if not found:
        raise argparse.ArgumentTypeError(f"Executable not found: {value}")
    # Keep rustup's cargo/rustc symlink names rather than resolving them to rustup.
    return Path(found).absolute()


def bounded_int(low, high):
    def parse(value):
        number = int(value)
        if not low <= number <= high:
            raise argparse.ArgumentTypeError(f"Expected {low}..{high}")
        return number

    return parse


def digest(path):
    checksum = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            checksum.update(block)
    return checksum.hexdigest()


def timings(log):
    text = log.read_text()
    matches = TIMING.findall(text)
    rss = MAX_RSS.findall(text)
    if len(matches) != 1 or len(rss) != 1:
        raise RuntimeError(f"Missing or ambiguous macOS /usr/bin/time result: {log}")
    real, user, system = map(float, matches[0])
    return dict(
        elapsed_seconds=real,
        user_seconds=user,
        system_seconds=system,
        cpu_seconds=round(user + system, 2),
        maximum_resident_set_bytes=int(rss[0]),
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--ref", default="HEAD", help="Git commit/ref to archive (default: HEAD)")
    parser.add_argument("--output", type=Path, required=True, help="New output directory; must not exist")
    parser.add_argument("--mbx", type=executable, required=True, help="Existing mbx binary; no installation")
    parser.add_argument("--cargo", type=executable, default="cargo", help="Plain Cargo executable")
    parser.add_argument("--rustc", type=executable, default="rustc", help="Rust compiler used by both tools")
    parser.add_argument("--registry-source", type=Path, default=Path.home() / ".cargo/registry")
    parser.add_argument("--trials", type=bounded_int(1, 5), default=3)
    parser.add_argument("--jobs", type=bounded_int(1, 8), default=2)
    parser.add_argument("--min-free-gib", type=float, default=2.0, help="Stop own build below this disk reserve")
    args = parser.parse_args()
    if sys.platform != "darwin":
        parser.error("This benchmark requires macOS /usr/bin/time -l and xcrun")
    if not 0.5 <= args.min_free_gib <= 20:
        parser.error("--min-free-gib must be between 0.5 and 20")

    repo = args.repo.resolve()
    root = args.output.resolve()
    if not (repo / "cli/Cargo.toml").is_file():
        parser.error("--repo must point to the BranchBox Cargo workspace")
    if root.exists():
        parser.error("--output already exists; choose a fresh directory to preserve prior results")

    env = os.environ.copy()
    for key in list(env):
        if key.startswith(("MBX_", "CARGO_", "RUST", "GITHUB_", "GITLAB_", "BUILDKITE_", "CI_")) or key in ("CI", "TF_BUILD", "JENKINS_URL"):
            del env[key]
    env["PATH"] = os.pathsep.join(dict.fromkeys([str(args.cargo.parent), str(args.rustc.parent), *env.get("PATH", "").split(os.pathsep)]))
    active_cargo = shutil.which("cargo", path=env["PATH"])
    if not active_cargo or not args.cargo.samefile(active_cargo):
        parser.error("The chosen --cargo must also resolve as cargo on its toolchain PATH")
    env.update(
        CARGO_HOME=str(root / "cargo-home"),
        CARGO_TERM_COLOR="never",
        CARGO_NET_OFFLINE="true",
        LC_ALL="C",
        RUSTC=str(args.rustc),
        SDKROOT=subprocess.check_output(["xcrun", "--sdk", "macosx", "--show-sdk-path"], text=True).strip(),
        MBX_CACHE_DIR=str(root / "preflight-cache"),
        MBX_SHIMS_DIR=str(root / "preflight-shims"),
        MBX_TARGET_VIEWS="0",
        MBX_GC_AUTO="0",
        MBX_LINKER="system",
        MBX_REMOTE_MODE="write-only",
        MBX_REMOTE_URL="",
        MBX_DISPLAY="plain",
        MBX_SUMMARY="full",
        MBX_INCREMENTAL="0",
        MBX_EAGER_INCREMENTAL="0",
        MBX_LEARNED_INCREMENTAL="1",
    )

    def isolation():
        # mbx has no alternate global config-file switch on macOS. Refuse a
        # global/workspace policy instead of changing HOME or its configuration.
        for path in (Path.home() / "Library/Application Support/mbx/config.toml", repo / ".mbx.toml"):
            if path.exists():
                raise RuntimeError(f"Unexpected mbx configuration: {path}")

    isolation()
    root.mkdir(parents=True, mode=0o700, exist_ok=False)
    logs = root / "logs"
    logs.mkdir()
    reserve = int(args.min_free_gib * GIB)
    records = []
    metadata = {}

    def save():
        temporary = root / "results.tmp"
        temporary.write_text(json.dumps(dict(metadata=metadata, builds=records), indent=2) + "\n")
        temporary.replace(root / "results.json")

    def remove_owned(path):
        path = path.resolve()
        if root not in path.parents:
            raise RuntimeError(f"Refusing cleanup outside {root}: {path}")
        if path.exists():
            # Restored OUT_DIR trees can contain read-only directories. Give
            # only our real directories enough permission for unlinking;
            # never chmod files or follow directory links into another tree.
            for directory, _subdirectories, _files in os.walk(path, followlinks=False):
                directory = Path(directory)
                mode = directory.lstat().st_mode
                if stat.S_ISDIR(mode):
                    os.chmod(directory, mode | stat.S_IWUSR | stat.S_IXUSR, follow_symlinks=False)
            shutil.rmtree(path)

    def checkpoint():
        isolation()
        free = shutil.disk_usage(root).free
        if free < reserve + GIB:
            raise RuntimeError(f"Only {free / GIB:.2f} GiB free; refusing another phase")
        return free

    def snapshot(destination):
        destination.mkdir(parents=True)
        subprocess.run(["tar", "-xf", str(root / "source.tar"), "-C", str(destination)], check=True)
        if (destination / ".mbx.toml").exists():
            raise RuntimeError("Archived source contains an mbx workspace policy")

    def smoke(target, words):
        binary = target / "debug/branchbox"
        title = "Alpha Beta Gamma Delta Epsilon Zeta"
        expected = "-".join(title.lower().split()[:words])
        result = subprocess.run([str(binary), "name", "generate", title], env=env, capture_output=True, text=True, check=True)
        if result.stdout.strip() != expected:
            raise RuntimeError(f"Compiled edit did not take effect: expected {expected}, got {result.stdout!r}")
        subprocess.run([str(binary), "name", "validate", "cache-smoke"], env=env, capture_output=True, check=True)
        invalid = subprocess.run([str(binary), "name", "validate", "INVALID"], env=env, capture_output=True)
        if invalid.returncode == 0:
            raise RuntimeError("CLI accepted an invalid feature name")
        subprocess.run([str(binary), "--help"], env=env, capture_output=True, check=True)
        return expected

    def stop(process):
        if process.poll() is None:
            os.killpg(process.pid, signal.SIGTERM)
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait()

    def build(trial, tool, stage, source, target, cache, words):
        free_before = checkpoint()
        child = env.copy()
        stem = f"trial-{trial}-{tool}-{stage}"
        stats = logs / (stem + "-stats.json")
        log = logs / (stem + ".log")
        child.update(CARGO_TARGET_DIR=str(target), MBX_CACHE_DIR=str(cache), MBX_SHIMS_DIR=str(cache / "shims"), MBX_STATS_REPORT=str(stats))
        command = [str(args.cargo if tool == "cargo" else args.mbx), "build", "--locked", "--offline", "-p", "branchbox-cli", "-j", str(args.jobs)]
        print(f"START {stem}: free={free_before / GIB:.2f} GiB", flush=True)
        started = time.monotonic()
        lowest_free = free_before
        with log.open("w") as output:
            process = subprocess.Popen(["/usr/bin/time", "-l", *command], cwd=source, env=child, stdout=output, stderr=subprocess.STDOUT, start_new_session=True)
            try:
                while process.poll() is None:
                    time.sleep(1)
                    lowest_free = min(lowest_free, shutil.disk_usage(root).free)
                    if lowest_free < reserve:
                        raise RuntimeError(f"Disk reserve reached during {stem}")
            except BaseException:
                stop(process)
                raise
        if process.returncode:
            raise RuntimeError(f"{stem} exited {process.returncode}; inspect {log}")
        entry = dict(trial=trial, tool=tool, stage=stage, argv=command, log=str(log), source=str(source), target=str(target), monitoring_seconds=round(time.monotonic() - started, 3), free_bytes_before=free_before, sampled_minimum_free_bytes=lowest_free)
        entry.update(timings(log))
        entry.update(expected_slug=smoke(target, words), smoke_passed=True)
        if tool == "mbx":
            report = json.loads(stats.read_text())
            if report.get("version") != 5:
                raise RuntimeError("Unexpected mbx statistics schema; review before comparing")
            if any(report.get(key, 0) for key in ("downloaded_bytes", "uploaded_bytes", "background_uploads", "background_upload_failures", "remote_blob_pack_uploads", "remote_manifest_lookups", "remote_action_lookups", "remote_blob_requests", "remote_blob_pack_requests", "remote_failures")):
                raise RuntimeError("Local-only benchmark recorded remote-cache activity")
            entry["mbx_stats"] = report
        records.append(entry)
        save()
        print(f"DONE {stem}: {entry['elapsed_seconds']:.2f}s real, smoke={entry['expected_slug']}", flush=True)

    def interrupted(_signum, _frame):
        raise KeyboardInterrupt

    signal.signal(signal.SIGTERM, interrupted)
    try:
        revision = subprocess.check_output(["git", "rev-parse", "--verify", "--end-of-options", args.ref + "^{commit}"], cwd=repo, text=True).strip()
        with (root / "source.tar").open("wb") as archive:
            subprocess.run(["git", "archive", revision], cwd=repo, stdout=archive, check=True)
        metadata.update(schema_version=1, started_utc=datetime.now(timezone.utc).isoformat(), status="running", revision=revision, cargo=str(args.cargo), rustc=str(args.rustc), mbx=str(args.mbx), mbx_sha256=digest(args.mbx), mbx_version=subprocess.check_output([str(args.mbx), "--version"], env=env, text=True).strip(), cargo_version=subprocess.check_output([str(args.cargo), "--version"], env=env, text=True).strip(), rustc_version=subprocess.check_output([str(args.rustc), "-Vv"], env=env, text=True).strip(), sdk=env["SDKROOT"], jobs=args.jobs, trials=args.trials, source_sha256=digest(root / "source.tar"), cpu_count=os.cpu_count(), memory_bytes=int(subprocess.check_output(["sysctl", "-n", "hw.memsize"], text=True)), disk_reserve_bytes=reserve, note="Sequential source snapshots; separate empty targets; fresh mbx cache per trial; Cargo default incremental vs mbx default learned incremental; system linker; timed builds offline. Compiler-time estimates are not elapsed savings. Maximum RSS is the macOS time statistic, not aggregate machine memory.")
        save()
        print(json.dumps(metadata, indent=2), flush=True)
        checkpoint()
        cargo_home = root / "cargo-home"
        cargo_home.mkdir()
        registry = args.registry_source.resolve()
        if registry.exists():
            # Copy registry material only: no config.toml, credentials, bin or
            # git credentials. Refuse links escaping this registry tree.
            for directory, subdirectories, files in os.walk(registry, followlinks=False):
                for name in subdirectories + files:
                    path = Path(directory) / name
                    if path.is_symlink() and not path.resolve().is_relative_to(registry):
                        raise RuntimeError(f"Registry symlink escapes its tree: {path}")
            shutil.copytree(registry, cargo_home / "registry", symlinks=False)
        seed = root / "seed-source"
        snapshot(seed)
        fetch_env = env.copy()
        fetch_env.pop("CARGO_NET_OFFLINE")
        with (logs / "fetch.log").open("w") as output:
            subprocess.run([str(args.cargo), "fetch", "--locked"], cwd=seed, env=fetch_env, stdout=output, stderr=subprocess.STDOUT, check=True)
        env["CARGO_TARGET_DIR"] = str(root / "doctor-target")
        doctor = subprocess.run([str(args.mbx), "doctor", "--json"], cwd=seed, env=env, capture_output=True, text=True)
        (logs / "doctor.json").write_text(doctor.stdout)
        (logs / "doctor.stderr.log").write_text(doctor.stderr)
        report = json.loads(doctor.stdout)
        if doctor.returncode or report.get("failures") != 0:
            raise RuntimeError("mbx doctor failed; inspect logs/doctor.json")
        remote = subprocess.run([str(args.mbx), "settings", "get", "remote.url"], cwd=seed, env=env, capture_output=True, text=True, check=True)
        if remote.stdout.strip():
            raise RuntimeError("Remote cache is configured; refusing a local-only comparison")
        remove_owned(seed)
        for trial in range(1, args.trials + 1):
            for tool in (["cargo", "mbx"] if trial % 2 else ["mbx", "cargo"]):
                area = root / f"trial-{trial}-{tool}"
                cache = area / "cache"
                source_a, source_b = area / "checkout-a", area / "checkout-b"
                target_a, target_b = area / "target-a", area / "target-b"
                snapshot(source_a)
                snapshot(source_b)
                build(trial, tool, "cold", source_a, target_a, cache, 3)
                remove_owned(target_a)
                remove_owned(source_a)
                build(trial, tool, "second-checkout", source_b, target_b, cache, 3)
                for old, new, stage in ((3, 4, "first-edit"), (4, 5, "later-edit")):
                    naming = source_b / "core/src/naming.rs"
                    before = f"const MAX_WORDS: usize = {old};"
                    text = naming.read_text()
                    if text.count(before) != 1:
                        raise RuntimeError("BranchBox naming source changed; review benchmark edit")
                    naming.write_text(text.replace(before, f"const MAX_WORDS: usize = {new};"))
                    build(trial, tool, stage, source_b, target_b, cache, new)
                remove_owned(area)
        metadata["status"] = "complete"
        save()
        print(f"COMPLETE: {len(records)} builds; results at {root / 'results.json'}", flush=True)
    except BaseException as error:
        metadata.update(status="interrupted" if isinstance(error, KeyboardInterrupt) else "failed", error=str(error))
        save()
        raise


if __name__ == "__main__":
    main()
