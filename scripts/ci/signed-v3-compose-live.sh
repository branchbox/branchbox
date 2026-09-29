#!/usr/bin/env bash
set -Eeuo pipefail

: "${BBX_BINARY:?exact-head binary required}"
: "${BBX_BINARY_SHA256:?verified artifact digest required}"
: "${BBX_RUNTIME_UID:?distinct runtime UID required}"
: "${BBX_RUNTIME_GID:?distinct runtime GID required}"
: "${BBX_DOCKER_GID:?Docker socket GID required}"
: "${BBX_RUNTIME_HOME:?runtime home required}"
test "$(id -u)" = 0
test "$BBX_RUNTIME_UID" != 0
test "$BBX_RUNTIME_UID" != 1000
test -d "$BBX_RUNTIME_HOME"
command -v devcontainer >/dev/null
docker compose version
BBX_LIVE_ROOT="$(mktemp -d /tmp/branchbox-live-compose.XXXXXXXX)"
trap 'rm -rf -- "$BBX_LIVE_ROOT"' EXIT
# The coding UID must be able to traverse the shared workspace parent. The
# signed run directory below remains 0700 and cannot be read by that UID.
chmod 0755 "$BBX_LIVE_ROOT"
install -m 0755 "$BBX_BINARY" "$BBX_LIVE_ROOT/branchbox"
test "$(sha256sum "$BBX_LIVE_ROOT/branchbox" | cut -d' ' -f1)" = "$BBX_BINARY_SHA256"
BBX_BINARY="$BBX_LIVE_ROOT/branchbox"
BBX_IMAGE="$(docker image inspect node:24-slim --format '{{index .RepoDigests 0}}')"
test -n "$BBX_IMAGE"
export BBX_LIVE_ROOT BBX_IMAGE

runtime_command() {
  setpriv --reuid "$BBX_RUNTIME_UID" --regid "$BBX_RUNTIME_GID" \
    --groups "$BBX_RUNTIME_GID,1000,$BBX_DOCKER_GID" \
    env HOME="$BBX_RUNTIME_HOME" "$@"
}

mutator_pid=""
compose_project=""
cleanup() {
  status="$?"
  trap - EXIT
  touch "$BBX_LIVE_ROOT/stop-mutation"
  if test -n "$mutator_pid"; then
    wait "$mutator_pid" 2>/dev/null || true
  fi
  if test "$status" != 0; then
    for log in start.stderr teardown.stderr; do
      if test -f "$BBX_LIVE_ROOT/$log"; then
        echo "----- $log -----" >&2
        cat "$BBX_LIVE_ROOT/$log" >&2
      fi
    done
  fi
  if test -n "${worktree:-}"; then
    docker ps -aq --filter "label=devcontainer.local_folder=$worktree" |
      while IFS= read -r container; do
        test -z "$container" || docker rm -f "$container" >/dev/null
      done || true
  fi
  if test -n "$compose_project"; then
    docker ps -aq --filter "label=com.docker.compose.project=$compose_project" |
      while IFS= read -r container; do
        test -z "$container" || docker rm -f "$container" >/dev/null
      done || true
    docker network ls -q --filter "label=com.docker.compose.project=$compose_project" |
      while IFS= read -r network; do
        test -z "$network" || docker network rm "$network" >/dev/null
      done || true
    docker volume ls -q --filter "label=com.docker.compose.project=$compose_project" |
      while IFS= read -r volume; do
        test -z "$volume" || docker volume rm "$volume" >/dev/null
      done || true
  fi
  rm -rf -- "$BBX_LIVE_ROOT" || true
  exit "$status"
}
trap cleanup EXIT

workspace="$BBX_LIVE_ROOT/workspace"
repository="$workspace/main"
worktree="$workspace/live"
run_root="$BBX_LIVE_ROOT/run"
mkdir -p "$repository/.devcontainer" "$run_root/materializations"
chmod 0700 "$run_root"
credential="$run_root/materializations/signed-fixture"
printf 'signed-fixture\n' > "$credential"
chmod 0644 "$credential"
credential_digest="$(sha256sum "$credential" | cut -d' ' -f1)"

git init -q "$repository"
git -C "$repository" config user.name 'BranchBox live check'
git -C "$repository" config user.email 'live-check@example.invalid'
cat > "$repository/.devcontainer/devcontainer.json" <<'JSON'
{
  "name": "branchbox-signed-v3-live",
  "dockerComposeFile": ["compose.yaml", "compose.extra.yaml"],
  "service": "app",
  "workspaceFolder": "/workspaces/live",
  "containerUser": "node",
  "remoteUser": "node",
  "userEnvProbe": "none",
  "updateRemoteUserUID": false,
  "forwardPorts": [31337]
}
JSON
cat > "$repository/.devcontainer/compose.yaml" <<'YAML'
name: hostile-compose-name
services:
  app:
    image: "${REPO_IMAGE:?discarded-source-image}"
    command: ["sleep", "3600"]
    environment:
      HOST_AUTH: "${HOST_AUTH:?discarded-source-environment}"
    volumes:
      - "${HOST_AUTH:?discarded-source-volume}:/host-auth:ro"
    ports:
      - "31337:3000"
  proxy:
    image: cloudflare/cloudflared:latest
YAML
cat > "$repository/.devcontainer/compose.extra.yaml" <<'YAML'
services:
  proxy:
    command: ["tunnel", "--token", "${TUNNEL_TOKEN:?discarded-connector-command}"]
YAML
printf '%s\n' 'COMPOSE_PROJECT_NAME=hostile-dotenv-name' 'COMPOSE_FILE=/does/not/exist.yaml' > "$repository/.env"
git -C "$repository" add .devcontainer .env
git -C "$repository" commit -qm 'Add hostile Compose fixture'
revision="$(git -C "$repository" rev-parse HEAD)"

jq -n \
  --arg workspace "$workspace" \
  --arg repository "$repository" \
  --arg revision "$revision" \
  --arg image "$BBX_IMAGE" \
  --arg credential "$credential" \
  --arg credential_digest "$credential_digest" \
  '{
    version:"3",run_id:"signed_v3_live_run",lease_id:"signed_v3_live_assignment",
    outer_runtime_id:"disposable_linux_ci",workspace:$workspace,
    repository:{path:$repository,revision:$revision},
    task_branch:"feature/live",tunnel_placement:"outer",published_ports:[],
    service_images:{app:$image},workspace_consumer:{uid:1000,gid:1000},
    leases:[
      {lease_id:"outer_tunnel",scope:"platform-tunnel",consumer:"outer-connector",materializations:[]},
      {lease_id:"bound_fixture",scope:"provider-credential",consumer:"coding-agent",
       expires_at:"2099-01-01T00:00:00Z",
       materializations:[{source_path:$credential,target_path:"/tmp/branchbox-signed-fixture",sha256:$credential_digest}]}
    ]
  }' > "$run_root/assignment.json"
chmod 0600 "$run_root/assignment.json"
# Setup is performed by the test supervisor; the actual BranchBox runtime
# owns the repository and assignment before it reads or writes either one.
chown -R "$BBX_RUNTIME_UID:$BBX_RUNTIME_GID" "$BBX_LIVE_ROOT"

echo "fixture worktree: $worktree"
echo "preloaded image: $BBX_IMAGE"
test "$(stat -c '%u:%a' "$run_root")" = "$BBX_RUNTIME_UID:700"
unset REPO_IMAGE HOST_AUTH TUNNEL_TOKEN

(
  count=0
  while ! test -e "$BBX_LIVE_ROOT/stop-mutation"; do
    stage="$(find "$run_root" -maxdepth 1 -type d -name 'branchbox-compose-*' -print -quit)"
    if test -n "$stage" && test -f "$stage/.devcontainer.json"; then
      break
    fi
    sleep 0.01
  done
  while ! test -e "$BBX_LIVE_ROOT/stop-mutation"; do
    if test -f "$worktree/.devcontainer/compose.yaml"; then
      if printf '%s\n' 'services:' '  app:' '    image: node:24-slim' '    volumes: ["/etc:/host-etc"]' '    ports: ["32345:3000"]' '    environment: {HOST_AUTH: "${HOST_AUTH:?late-source}"}' |
        setpriv --reuid 1000 --regid 1000 --clear-groups tee "$worktree/.devcontainer/compose.yaml" >/dev/null 2>&1; then
        count=$((count + 1))
      fi
    fi
    sleep 0.01
  done
  printf '%s\n' "$count" > "$BBX_LIVE_ROOT/mutation-count"
) &
mutator_pid="$!"

set +e
runtime_command timeout 180s "$BBX_BINARY" feature start live --repo "$repository" --runtime in-guest --runtime-manifest "$run_root/assignment.json" --allow-container --minimal --json > "$BBX_LIVE_ROOT/start.stdout" 2> "$BBX_LIVE_ROOT/start.stderr"
start_status="$?"
set -e
touch "$BBX_LIVE_ROOT/stop-mutation"
wait "$mutator_pid"
echo "Hostile repository Compose rewrites: $(cat "$BBX_LIVE_ROOT/mutation-count")"
if test "$start_status" != 0; then
  exit "$start_status"
fi
cat "$BBX_LIVE_ROOT/start.stdout"

stage="$(find "$run_root" -maxdepth 1 -type d -name 'branchbox-compose-*' -print -quit)"
test -n "$stage"
test "$(stat -c '%u:%a' "$stage")" = "$BBX_RUNTIME_UID:700"
test "$stage" != "$worktree"
test "$(cat "$BBX_LIVE_ROOT/mutation-count")" -gt 0
grep -Fq '/etc:/host-etc' "$worktree/.devcontainer/compose.yaml"
if grep -R -F '${' "$stage"; then
  echo "Repository interpolation survived in private CLI inputs" >&2
  exit 1
fi
test ! -e "$worktree/.devcontainer/.devcontainer.json"
if setpriv --reuid 1000 --regid 1000 --clear-groups test -r "$stage/.devcontainer.json"; then
  echo "Workspace consumer can read private CLI inputs" >&2
  exit 1
fi
printf 'consumer-created\n' |
  setpriv --reuid 1000 --regid 1000 --clear-groups tee "$worktree/consumer-created.txt" >/dev/null
test "$(stat -c %u "$worktree/consumer-created.txt")" = 1000
test "$(stat -c %a "$worktree/consumer-created.txt")" = 660
setpriv --reuid 1000 --regid 1000 --clear-groups sh -c \
  'umask 077; mkdir -p "$1/inner"; printf "nested consumer file\n" > "$1/inner/file"' \
  sh "$worktree/consumer-private"
test "$(stat -c %u "$worktree/consumer-private/inner/file")" = 1000
private_mode="$(stat -c %a "$worktree/consumer-private")"
inner_mode="$(stat -c %a "$worktree/consumer-private/inner")"
test "${private_mode: -3}" = 770
test "${inner_mode: -3}" = 770
test "$(stat -c %a "$worktree/consumer-private/inner/file")" = 660
public_mode="$(stat -c %a "$worktree/.devcontainer")"
test "${public_mode: -3}" = 775
setpriv --reuid 1000 --regid 1000 --clear-groups sh -c \
  'umask 077; printf "public-parent file\n" > "$1/consumer-file"; mkdir "$1/consumer-dir"' \
  sh "$worktree/.devcontainer"
test "$(stat -c %a "$worktree/.devcontainer/consumer-file")" = 660
public_child_mode="$(stat -c %a "$worktree/.devcontainer/consumer-dir")"
test "${public_child_mode: -3}" = 770
container_ids="$(docker ps -q --filter "label=devcontainer.local_folder=$worktree")"
test "$(printf '%s\n' "$container_ids" | sed '/^$/d' | wc -l)" = 1
container_id="$container_ids"
docker inspect "$container_id" > "$BBX_LIVE_ROOT/container-inspect.json"
compose_project="$(jq -r '.[0].Config.Labels["com.docker.compose.project"] // empty' "$BBX_LIVE_ROOT/container-inspect.json")"
test -n "$compose_project"
managed_project="$(jq -r '.compose_project_name // empty' "$BBX_LIVE_ROOT/start.stdout")"
test -n "$managed_project"
test "$compose_project" = "$managed_project"
image_id="$(docker image inspect "$BBX_IMAGE" --format '{{.Id}}')"
test "$(docker inspect "$container_id" --format '{{.Image}}')" = "$image_id"
configured_user="$(docker inspect "$container_id" --format '{{.Config.User}}')"
test -n "$configured_user"
test "$configured_user" != root
test "$configured_user" != 0
container_uid="$(docker exec "$container_id" id -u)"
container_gid="$(docker exec "$container_id" id -g)"
test "$container_uid" = 1000
test "$container_gid" = 1000
test "$(docker exec "$container_id" cat /tmp/branchbox-signed-fixture)" = signed-fixture
coding_uid="$(runtime_command env COMPOSE_PROJECT_NAME="$managed_project" devcontainer exec --workspace-folder "$worktree" --config "$stage/.devcontainer.json" id -u)"
test "$coding_uid" = 1000
jq -e --arg worktree "$worktree" --arg git "$repository/.git" --arg credential "$credential" '
  .[0] as $container |
  ($container.Mounts | length) == 3 and
  ([$container.Mounts[].Source] | sort) == ([$worktree,$git,$credential] | sort) and
  ([$container.Mounts[] | select(.Source == $credential) | .RW] == [false]) and
  ($container.HostConfig.PortBindings | length) == 0 and
  ($container.Config.Env | map(select(test("HOST_AUTH|TUNNEL_TOKEN"))) | length) == 0
' "$BBX_LIVE_ROOT/container-inspect.json"
echo "live-start-boundary=verified"

# The coding UID can change the shared Git config and worktree attributes. An unguarded status,
# hook, or clean-filter invocation during teardown would execute this script as the runtime UID.
git_attack_script="$worktree/git-attack.sh"
cat > "$git_attack_script" <<EOF
#!/bin/sh
printf invoked >> "$BBX_LIVE_ROOT/git-attack-marker"
cat
EOF
chown 1000:1000 "$git_attack_script"
chmod 0750 "$git_attack_script"
mkdir "$worktree/git-hooks"
cp "$git_attack_script" "$worktree/git-hooks/reference-transaction"
chmod 0750 "$worktree/git-hooks/reference-transaction"
chown -R 1000:1000 "$worktree/git-hooks"
setpriv --reuid 1000 --regid 1000 --clear-groups test -w "$repository/.git/config"
setpriv --reuid 1000 --regid 1000 --clear-groups git -C "$worktree" config --file "$repository/.git/config" core.fsmonitor "$git_attack_script"
setpriv --reuid 1000 --regid 1000 --clear-groups git -C "$worktree" config --file "$repository/.git/config" core.hooksPath "$worktree/git-hooks"
setpriv --reuid 1000 --regid 1000 --clear-groups git -C "$worktree" config --file "$repository/.git/config" filter.attack.clean "$git_attack_script"
printf '.env filter=attack\n' |
  setpriv --reuid 1000 --regid 1000 --clear-groups tee "$worktree/.gitattributes" >/dev/null
setpriv --reuid 1000 --regid 1000 --clear-groups sh -c \
  'sed "s/hostile-dotenv-name/hostile-dotenv-namE/" "$1/.env" > "$1/.env.tmp"; mv "$1/.env.tmp" "$1/.env"' \
  sh "$worktree"
runtime_command "$git_attack_script" </dev/null
test -e "$BBX_LIVE_ROOT/git-attack-marker"
rm "$BBX_LIVE_ROOT/git-attack-marker"
test ! -e "$BBX_LIVE_ROOT/git-attack-marker"
set +e
runtime_command "$BBX_BINARY" feature teardown live --repo "$repository" --delete-branch --allow-container --json > "$BBX_LIVE_ROOT/refused-teardown.stdout" 2> "$BBX_LIVE_ROOT/refused-teardown.stderr"
refused_status="$?"
set -e
test "$refused_status" != 0
grep -Fq 'requires --force' "$BBX_LIVE_ROOT/refused-teardown.stderr"
test -d "$worktree"
test ! -e "$BBX_LIVE_ROOT/git-attack-marker"

runtime_command timeout 180s "$BBX_BINARY" feature teardown live --repo "$repository" --force --delete-branch --force-delete-branch --allow-container --json > "$BBX_LIVE_ROOT/teardown.stdout" 2> "$BBX_LIVE_ROOT/teardown.stderr"
cat "$BBX_LIVE_ROOT/teardown.stdout"
jq -e '.worktree_removed == true and .branch_deleted == true' "$BBX_LIVE_ROOT/teardown.stdout"
test ! -e "$BBX_LIVE_ROOT/git-attack-marker"
jq -e '.runtime_teardown.verified == true and .runtime_teardown.residue_free == true' "$BBX_LIVE_ROOT/teardown.stdout"
test ! -e "$worktree"
test ! -e "$stage"
test ! -e "$credential"
test -z "$(docker ps -aq --filter "label=devcontainer.local_folder=$worktree")"
test -z "$(docker ps -aq --filter "label=com.docker.compose.project=$compose_project")"
test -z "$(docker network ls -q --filter "label=com.docker.compose.project=$compose_project")"
test -z "$(docker volume ls -q --filter "label=com.docker.compose.project=$compose_project")"
test -z "$(runtime_command git -C "$repository" branch --list feature/live)"
echo "provider-teardown=verified"
