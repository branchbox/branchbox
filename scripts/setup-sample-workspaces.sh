#!/usr/bin/env bash

set -euo pipefail

usage() {
    cat <<'USAGE'
Usage: setup-sample-workspaces.sh [--force] [sample...]

Copy sample project templates into test/workspaces/local/ and bootstrap git
repositories for devcontainer smoke testing.

Options:
  --force       Recreate selected samples even if a local copy already exists.
  -h, --help    Show this help message.

Arguments:
  sample        Optional list of template names (e.g. rust-cli). Defaults to all.
USAGE
}

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEMPLATE_DIR="${ROOT_DIR}/test/workspaces/templates"
LOCAL_DIR="${ROOT_DIR}/test/workspaces/local"

force=false
declare -a selected_templates=()

while (($#)); do
    case "$1" in
        --force)
            force=true
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        -*)
            echo "Unknown option: $1" >&2
            usage
            exit 1
            ;;
        *)
            selected_templates+=("$1")
            ;;
    esac
    shift
done

if [ ! -d "$TEMPLATE_DIR" ]; then
    echo "Template directory not found: $TEMPLATE_DIR" >&2
    exit 1
fi

if [ ${#selected_templates[@]} -eq 0 ]; then
    while IFS= read -r dir; do
        [ -z "$dir" ] && continue
        selected_templates+=("$(basename "$dir")")
    done < <(find "$TEMPLATE_DIR" -mindepth 1 -maxdepth 1 -type d -print | LC_ALL=C sort)
fi

mkdir -p "$LOCAL_DIR"

for template in "${selected_templates[@]}"; do
    template_path="${TEMPLATE_DIR}/${template}"
    if [ ! -d "$template_path" ]; then
        echo "⚠️  Template '${template}' not found in ${TEMPLATE_DIR}; skipping."
        continue
    fi

    target_path="${LOCAL_DIR}/${template}"

    stack=""
    description=""
    metadata_file="${template_path}/template.json"
    if [ -f "$metadata_file" ]; then
        if command -v python3 >/dev/null 2>&1; then
            stack="$(python3 -c 'import json,sys; data=json.load(open(sys.argv[1])); print(data.get("stack",""))' "$metadata_file" 2>/dev/null || true)"
            description="$(python3 -c 'import json,sys; data=json.load(open(sys.argv[1])); print(data.get("description",""))' "$metadata_file" 2>/dev/null || true)"
        fi
        if [ -z "$stack" ] && command -v python >/dev/null 2>&1; then
            stack="$(python -c 'import json,sys; data=json.load(open(sys.argv[1])); print(data.get("stack",""))' "$metadata_file" 2>/dev/null || true)"
            description="$(python -c 'import json,sys; data=json.load(open(sys.argv[1])); print(data.get("description",""))' "$metadata_file" 2>/dev/null || true)"
        fi
    fi

    if [ -z "$stack" ]; then
        echo "⚠️  Template '${template}' is missing stack metadata; defaulting to 'rust'."
        stack="rust"
    fi

    if [ -e "$target_path" ]; then
        if [ "$force" = true ]; then
            rm -rf "$target_path"
        else
            echo "ℹ️  Sample '${template}' already exists at ${target_path}; skipping (use --force to recreate)."
            continue
        fi
    fi

    mkdir -p "$target_path"
    cp -a "${template_path}/." "$target_path/"

    pushd "$target_path" >/dev/null
    if [ ! -d .git ]; then
        git init -b main >/dev/null 2>&1
        git config user.name "BranchBox Tester"
        git config user.email "branchbox@example.com"
        git add . >/dev/null 2>&1
        git commit -m "Initial commit" >/dev/null 2>&1
    fi
    popd >/dev/null

    cat <<EOF
✅ Prepared sample '${template}'
   Path: ${target_path}
   Stack: ${stack}
   Description: ${description:-n/a}
   Next steps:
     cd ${target_path}
     BRANCHBOX_SKIP_HOST_VALIDATION=1 branchbox init --stack ${stack}
     branchbox feature start "devcontainer-smoke"
     # Open both the main repo and feature worktree in your editor to confirm devcontainer propagation.
EOF
done
