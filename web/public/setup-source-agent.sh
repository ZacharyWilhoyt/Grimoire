#!/usr/bin/env bash
set -euo pipefail

source_url='https://github.com/ZacharyWilhoyt/Grimoire/blob/codex/dedicated-music-page/web/public/setup-source-agent.sh'
printf 'Setup script source: %s\n' "$source_url"
read -r -p 'Have you read and understood this script on GitHub? [y/N] ' answer
case "$answer" in
  y|Y|yes|YES) ;;
  *) printf 'Setup cancelled.\n'; exit 0 ;;
esac

coordinator=${1:-https://grimoire.devmage.dev}
if [[ ! "$coordinator" =~ ^https://[a-zA-Z0-9.-]+(:[0-9]+)?$ ]]; then
  printf 'Enter a coordinator HTTPS origin such as https://grimoire.devmage.dev.\n' >&2
  exit 1
fi
image="${coordinator#https://}/grimoire/source-agent:latest"
container=grimoire-source-agent
volume=grimoire-agent
config_parent=${XDG_CONFIG_HOME:-"$HOME/.config"}
config_dir="$config_parent/grimoire-source-agent"

for tool in docker python3; do
  if ! command -v "$tool" >/dev/null; then
    printf 'Install %s on this machine before running setup.\n' "$tool" >&2
    exit 1
  fi
done
docker info --format '{{.ServerVersion}}' >/dev/null
if docker container inspect "$container" >/dev/null 2>&1 || [[ -e "$config_dir" || -L "$config_dir" ]]; then
  printf 'An agent container or setup directory already exists. Configuration: %s\n' "$config_dir" >&2
  exit 1
fi
if [[ "$config_dir" == *,* || "$config_dir" != /* ]]; then
  printf 'The configuration directory must be absolute and contain no commas.\n' >&2
  exit 1
fi

read -r -p 'Agent name [My agent]: ' agent_name
agent_name=${agent_name:-My agent}
if [[ "$agent_name" =~ [[:cntrl:]] || "$agent_name" =~ ^[[:space:]] || "$agent_name" =~ [[:space:]]$ || ${#agent_name} -gt 80 ]]; then
  printf 'Use an agent name of at most 80 characters with no surrounding whitespace.\n' >&2
  exit 1
fi
mounts=()
sources=()
directories=()
while true; do
  read -r -p 'Media directory on this machine (blank when finished): ' directory
  if [[ -z "$directory" ]]; then
    if (( ${#directories[@]} )); then break; fi
    printf 'Enter at least one directory.\n'
    continue
  fi
  if ! directory=$(cd -- "$directory" >/dev/null 2>&1 && pwd -P) || [[ "$directory" == / || "$directory" == *,* ]]; then
    printf 'Enter an existing media directory. Use a path without commas.\n'
    continue
  fi
  duplicate=false
  for existing in "${directories[@]}"; do
    if [[ "$directory" == "$existing" ]]; then duplicate=true; fi
  done
  if "$duplicate"; then
    printf 'That directory is already selected.\n'
    continue
  fi
  default_name=${directory##*/}
  read -r -p "Source display name [$default_name]: " source_name
  source_name=${source_name:-$default_name}
  if [[ "$source_name" =~ [[:cntrl:]] || "$source_name" =~ ^[[:space:]] || "$source_name" =~ [[:space:]]$ || ${#source_name} -gt 120 ]]; then
    printf 'Use a source name of at most 120 characters with no surrounding whitespace.\n'
    continue
  fi
  mount_path="/media/source-$((${#directories[@]} + 1))"
  directories+=("$directory")
  mounts+=(--mount "type=bind,source=$directory,target=$mount_path,readonly")
  sources+=("$source_name" "$mount_path")
  if (( ${#directories[@]} == 32 )); then break; fi
done

docker pull "$image"
for ((index=1; index<=${#directories[@]}; index++)); do
  if ! docker run --rm --network none --read-only --cap-drop ALL --entrypoint sh \
    "${mounts[@]}" "$image" -c 'test -r "$1" && test -x "$1"' sh "/media/source-$index"; then
    printf 'The agent cannot read or traverse %s. Give UID 10001 access and run setup again.\n' "${directories[index-1]}" >&2
    exit 1
  fi
done

printf 'Create a single-use token in Grimoire under Source Agents. Paste it at the next prompt and press Enter. The token stays hidden while you type.\n'
docker run --rm -it --read-only --cap-drop ALL --security-opt no-new-privileges \
  -v "$volume:/var/lib/grimoire-agent" "$image" enroll "$coordinator" "$agent_name"

mkdir -p -- "$config_parent"
mkdir -m 755 -- "$config_dir"
if ! python3 - "$config_dir/sources.json" "${sources[@]}" <<'PY'
import json
import os
import sys
import uuid

path, *pairs = sys.argv[1:]
sources = [
    dict(id=str(uuid.uuid4()), name=name, root=root, published=True, grants=[])
    for name, root in zip(pairs[::2], pairs[1::2])
]
with open(path, "x", encoding="utf-8") as output:
    json.dump({"sources": sources}, output, ensure_ascii=False, indent=2)
    output.write("\n")
os.chmod(path, 0o644)
PY
then
  printf 'Could not write the configuration at %s. The enrolled identity is in Docker volume %s.\n' "$config_dir" "$volume" >&2
  exit 1
fi

docker run -d --name "$container" --restart unless-stopped --read-only --cap-drop ALL \
  --security-opt no-new-privileges --pids-limit 64 --memory 512m \
  -e GRIMOIRE_AGENT_CONFIG=/etc/grimoire-agent/sources.json \
  -v "$volume:/var/lib/grimoire-agent" \
  --mount "type=bind,source=$config_dir,target=/etc/grimoire-agent,readonly" \
  "${mounts[@]}" "$image" run
printf 'Agent started. Refresh Source Agents in Grimoire.\nConfiguration: %s/sources.json\nLogs: docker logs %s\n' "$config_dir" "$container"
