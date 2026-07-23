#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "OpenStrike's Linux package must be built on Linux." >&2
  exit 1
fi

project_dir="$(cd "$(dirname "$0")/.." && pwd)"
package_root="$project_dir/dist/OpenStrike-linux"
archive="$project_dir/dist/OpenStrike-linux-x86_64.tar.gz"

cd "$project_dir"
bun run build:desktop
cargo build --release -p openstrike

rm -rf "$package_root"
mkdir -p "$package_root/assets" "$package_root/ui"
cp target/release/openstrike "$package_root/openstrike-bin"
cp -R assets/maps assets/support assets/models "$package_root/assets/"
cp dist/pocket/desktop/openstrike.js dist/pocket/desktop/openstrike.pak "$package_root/ui/"
cp crates/openstrike-vita/static/sce_sys/icon0.png "$package_root/icon.png"

cat > "$package_root/openstrike" <<'LAUNCHER'
#!/usr/bin/env bash
set -euo pipefail
app_dir="$(cd "$(dirname "$0")" && pwd)"
cd "$app_dir"
export OPENSTRIKE_ASSETS="$app_dir/assets"
export OPENSTRIKE_UI_DIST="$app_dir/ui"
export RUST_LOG="${RUST_LOG:-openstrike=info,pocket3d=info}"
exec "$app_dir/openstrike-bin" --maps-dir "$app_dir/assets" --map open_arena "$@"
LAUNCHER

cat > "$package_root/install.sh" <<'INSTALLER'
#!/usr/bin/env bash
set -euo pipefail
source_dir="$(cd "$(dirname "$0")" && pwd)"
install_dir="${OPENSTRIKE_INSTALL_DIR:-$HOME/.local/share/openstrike}"
bin_dir="$HOME/.local/bin"
desktop_dir="$HOME/.local/share/applications"

mkdir -p "$install_dir" "$bin_dir" "$desktop_dir"
cp -R "$source_dir/." "$install_dir/"
ln -sfn "$install_dir/openstrike" "$bin_dir/openstrike"
sed \
  -e "s|^Exec=.*|Exec=$install_dir/openstrike|" \
  -e "s|^Icon=.*|Icon=$install_dir/icon.png|" \
  "$source_dir/OpenStrike.desktop" > "$desktop_dir/OpenStrike.desktop"
chmod +x "$install_dir/openstrike" "$install_dir/openstrike-bin"
echo "OpenStrike installed. Launch it from the applications menu or run: $bin_dir/openstrike"
INSTALLER

cp OpenStrike.desktop "$package_root/OpenStrike.desktop"
chmod +x "$package_root/openstrike" "$package_root/openstrike-bin" "$package_root/install.sh"
rm -f "$archive"
tar -C "$project_dir/dist" -czf "$archive" OpenStrike-linux
echo "share: $archive"
