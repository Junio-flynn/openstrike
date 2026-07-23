#!/usr/bin/env bash
set -u

project_dir="$(cd "$(dirname "$0")/.." && pwd)"
game_bin="$project_dir/target/release/openstrike"
bun_bin="$(command -v bun 2>/dev/null || true)"
if [[ -z "$bun_bin" && -x "$HOME/.bun/bin/bun" ]]; then
  bun_bin="$HOME/.bun/bin/bun"
fi
config_dir="${XDG_CONFIG_HOME:-$HOME/.config}/openstrike"
config_file="$config_dir/maps-dir"
test_maps="$project_dir/.local/test-maps"
built_in_maps="$project_dir/assets"

maps_dir="${OPENSTRIKE_MAPS:-}"
if [[ -z "$maps_dir" && -f "$config_file" ]]; then
  IFS= read -r maps_dir < "$config_file"
fi

first_map() {
  local root="$1"
  local candidates=()
  if [[ -f "$root/maps/open_arena.bsp" ]]; then
    printf '%s\n' "$root/maps/open_arena.bsp"
    return
  fi
  if [[ -f "$root/open_arena.bsp" ]]; then
    printf '%s\n' "$root/open_arena.bsp"
    return
  fi
  shopt -s nullglob
  candidates=("$root"/maps/*.bsp "$root"/*.bsp)
  shopt -u nullglob
  if (( ${#candidates[@]} > 0 )); then
    printf '%s\n' "${candidates[0]}"
  fi
}

map_file=""
if [[ -n "$maps_dir" ]]; then
  map_file="$(first_map "$maps_dir")"
elif [[ -n "$(first_map "$built_in_maps")" ]]; then
  maps_dir="$built_in_maps"
  map_file="$(first_map "$maps_dir")"
elif [[ -n "$(first_map "$test_maps")" ]]; then
  maps_dir="$test_maps"
  map_file="$(first_map "$maps_dir")"
fi

if [[ -z "$map_file" ]]; then
  maps_dir="$(kdialog --getexistingdirectory "$HOME" --title "Choose OpenStrike map-data folder")"
  if [[ -z "$maps_dir" ]]; then
    exit 0
  fi
  map_file="$(first_map "$maps_dir")"
  if [[ -z "$map_file" ]]; then
    kdialog --error "That folder does not contain a .bsp map. Choose the folder containing maps/ and support/." --title "OpenStrike"
    exit 1
  fi
  mkdir -p "$config_dir"
  printf '%s\n' "$maps_dir" > "$config_file"
fi

cd "$project_dir" || exit 1
export OPENSTRIKE_UI_DIST="$project_dir/dist/pocket/desktop"
export RUST_LOG="${RUST_LOG:-openstrike=info,pocket3d=info}"
log_file="$config_dir/openstrike.log"
mkdir -p "$config_dir"

# This launcher is also used during development. Never silently run an old
# native host against a new UI bundle: missing surface fields can make menu
# actions disappear even though the TypeScript was rebuilt.
if [[ ! -x "$game_bin" ]] \
  || [[ -n "$(find crates/openstrike crates/openstrike-core -type f -newer "$game_bin" -print -quit)" ]] \
  || [[ Cargo.toml -nt "$game_bin" ]]; then
  if ! cargo build --release -p openstrike >>"$log_file" 2>&1; then
    kdialog --error "OpenStrike failed to rebuild. See $log_file" --title "OpenStrike"
    exit 1
  fi
fi

ui_js="$OPENSTRIKE_UI_DIST/openstrike.js"
if [[ ! -f "$ui_js" ]] \
  || [[ -n "$(find game -type f -newer "$ui_js" -print -quit)" ]]; then
  if [[ -z "$bun_bin" ]]; then
    kdialog --error "OpenStrike needs Bun to rebuild its interface, but Bun was not found." --title "OpenStrike"
    exit 1
  fi
  if ! "$bun_bin" run build:desktop >>"$log_file" 2>&1; then
    kdialog --error "OpenStrike's interface failed to rebuild. See $log_file" --title "OpenStrike"
    exit 1
  fi
fi

exec "$game_bin" --maps-dir "$maps_dir" --map "$map_file" >>"$log_file" 2>&1
