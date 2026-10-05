#!/usr/bin/env bash
set -euo pipefail

# Capture the JSON a real branchbox CLI prints for the commands the Mac app runs, from a disposable git repo,
# for LiveFixtureDecodeTests (BRANCHBOX_LIVE_FIXTURES=<outdir>; DESIGN §12.2, §13.2). Docker-free:
# `feature start --minimal --skip-module tunnel`. Paths are scrubbed and the temp repo is removed afterwards.

usage() {
  cat <<'EOF'
Usage: scripts/macos-capture-fixtures.sh <cli> <outdir>

Runs <cli> in a temporary git repo and writes one file per command to <outdir>:
  <name>.json     stdout (when it printed anything)
  <name>.stderr   stderr
  manifest.json   {"cli" (file name), "cli_version", "entries": [{"name", "kind", "argv", "exit", "stdout", "stderr"}]}
                  `kind` names the shape to decode stdout as: version, feature_list, start_summary,
                  exec_result, teardown_plan, teardown_summary or error_envelope. Entries the CLI does not
                  support (exit 2 on an unknown subcommand or flag) are listed with "skipped": true.

Captured: version --json (if supported), feature list --json, feature list --all --json,
feature start --minimal --skip-module tunnel --json, feature exec --json (ok, and exit 3),
feature teardown --dry-run --json (if supported), feature teardown --json, feature list --all --json
afterwards, and the error output for a missing feature.

The temp repo's path is replaced by /tmp/branchbox-live and $HOME by /Users/user in every file.
Fails if list, start or exec ok fails; the other exit codes are recorded, not judged.
EOF
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then usage; exit 0; fi
[[ $# -eq 2 ]] || { usage >&2; exit 2; }

cli="$1"
out="$2"
[[ -x "$cli" && -f "$cli" ]] || cli="$(command -v "$cli" 2>/dev/null || true)"
[[ -n "$cli" && -x "$cli" ]] || { echo "error: $1 is not an executable branchbox" >&2; exit 1; }
cli="$(cd "$(dirname "$cli")" && pwd -P)/$(basename "$cli")"

mkdir -p "$out"
out="$(cd "$out" && pwd -P)"
rm -f "$out"/*.json "$out"/*.stderr

container="$(mktemp -d "${TMPDIR:-/tmp}/branchbox-live.XXXXXX")"
container="$(cd "$container" && pwd -P)"
repo="${container}/main"
cleanup() { rm -rf "$container"; }
trap cleanup EXIT

# The repo: main branch, one commit, and the .gitignore entries `branchbox init` adds, so a fresh feature's
# only untracked files are the ones BranchBox generates (as TempRepo.branchBoxIgnores in the Swift tests).
git_quiet() { git -C "$repo" -c user.email=fixtures@example.com -c user.name="BranchBox Fixtures" -c commit.gpgsign=false "$@" >/dev/null; }
mkdir -p "$repo"
git_quiet init -q -b main
printf 'hello\n' > "${repo}/README.md"
cat > "${repo}/.gitignore" <<'EOF'
.DS_Store
.branchbox/registry.json
.branchbox/secure/
.branchbox/runtime/
.branchbox/devcontainer-sync/
.branchbox/.registry.*.tmp
.branchbox/.lock
.devcontainer/.branchbox.env
.devcontainer/.cloudflared.env
.devcontainer/.branchbox-sbx-compose.yaml
.devcontainer/.devcontainer.json
.devcontainer/.github-token.env
.devcontainer/.git-signing-key
.devcontainer/.gitconfig.env
.branchbox.env
.env
.env.local
EOF
git_quiet add -A
git_quiet commit -q -m init

json_string() {
  local s="$1"
  s="${s//\\/\\\\}"; s="${s//\"/\\\"}"; s="${s//$'\t'/\\t}"; s="${s//$'\n'/\\n}"
  printf '"%s"' "$s"
}

entries=()
last_exit=0

# capture NAME KIND [--optional] -- ARGV...   Runs the CLI in the repo with stdin closed.
capture() {
  local name="$1" kind="$2" optional=0 status=0 argv_json="" arg
  shift 2
  if [[ "$1" == "--optional" ]]; then optional=1; shift; fi
  [[ "$1" == "--" ]] && shift
  (cd "$repo" && "$cli" "$@" </dev/null >"${out}/${name}.json" 2>"${out}/${name}.stderr") || status=$?
  last_exit=$status
  for arg in "$@"; do argv_json+="${argv_json:+, }$(json_string "$arg")"; done
  if [[ "$optional" == 1 && "$status" == 2 ]]; then
    rm -f "${out}/${name}.json"
    entries+=("{\"name\": \"${name}\", \"kind\": \"${kind}\", \"argv\": [${argv_json}], \"exit\": 2, \"skipped\": true, \"stderr\": \"${name}.stderr\"}")
    echo "  ${name}: not supported by this CLI (exit 2)" >&2
    return 0
  fi
  local stdout_field="null"
  if [[ -s "${out}/${name}.json" ]]; then stdout_field="\"${name}.json\""; else rm -f "${out}/${name}.json"; fi
  entries+=("{\"name\": \"${name}\", \"kind\": \"${kind}\", \"argv\": [${argv_json}], \"exit\": ${status}, \"stdout\": ${stdout_field}, \"stderr\": \"${name}.stderr\"}")
  echo "  ${name}: exit ${status}" >&2
}

require_ok() {
  [[ "$last_exit" == 0 ]] && return 0
  echo "error: $1 failed (exit ${last_exit}); stderr:" >&2
  sed 's/^/  /' "${out}/$1.stderr" >&2
  exit 1
}

cli_version="$("$cli" --version 2>/dev/null </dev/null | head -n 1 || true)"
echo "Capturing ${cli_version:-branchbox} output into ${out}" >&2

capture version version --optional -- version --json
capture feature_list feature_list -- feature list --json --repo "$repo"
require_ok feature_list
capture feature_list_all feature_list -- feature list --json --repo "$repo" --all
require_ok feature_list_all
capture start_minimal start_summary -- feature start live --repo "$repo" --json --runtime container --minimal --skip-module tunnel
require_ok start_minimal
capture feature_list_started feature_list -- feature list --json --repo "$repo"
capture exec_ok exec_result -- feature exec --repo "$repo" --json live -- echo hi
require_ok exec_ok
capture exec_exit3 exec_result -- feature exec --repo "$repo" --json live -- sh -c 'echo out; echo err >&2; exit 3'
capture teardown_plan teardown_plan --optional -- feature teardown live --repo "$repo" --dry-run --json --branch-prefix feature --delete-branch
capture teardown teardown_summary -- feature teardown live --repo "$repo" --json --branch-prefix feature --delete-branch
capture feature_list_all_after feature_list -- feature list --json --repo "$repo" --all
capture missing_feature error_envelope -- feature teardown nope --repo "$repo" --json --branch-prefix feature --keep-branch

{
  printf '{\n  "cli": %s,\n  "cli_version": %s,\n  "entries": [\n' "$(json_string "$(basename "$cli")")" "$(json_string "$cli_version")"
  for i in "${!entries[@]}"; do
    printf '    %s%s\n' "${entries[$i]}" "$([[ $i -lt $((${#entries[@]} - 1)) ]] && echo ,)"
  done
  printf '  ]\n}\n'
} > "${out}/manifest.json"

# Scrub: the temp repo (also as /private/var or /var), then $HOME. Literal replacement via perl \Q…\E.
scrub_from=("$container" "${container#/private}" "$HOME")
scrub_to=("/tmp/branchbox-live" "/tmp/branchbox-live" "/Users/user")
for file in "$out"/*.json "$out"/*.stderr; do
  [[ -e "$file" ]] || continue
  for i in "${!scrub_from[@]}"; do
    FROM="${scrub_from[$i]}" TO="${scrub_to[$i]}" perl -pi -e 's/\Q$ENV{FROM}\E/$ENV{TO}/g' "$file"
  done
done
if grep -rlF -e "$container" -e "$HOME/" "$out" >/dev/null 2>&1; then
  echo "error: unscrubbed paths remain in ${out}" >&2
  exit 1
fi

echo "Wrote $(ls "$out" | wc -l | tr -d ' ') files to ${out}" >&2
echo "$out"
