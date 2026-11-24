#!/bin/sh
# Installs Strata: curl -fsSL https://strata-lang.dev/install.sh | sh
#
# It downloads the release for this machine, checks it against its .sha256,
# and puts it in ~/.strata (bin/strata and lib/). Running it again upgrades in place.
#
# Settings, all optional:
#   STRATA_VERSION=v0.1      a release tag (default: the latest release)
#   STRATA_INSTALL=DIR       where to install (default: ~/.strata)
#   STRATA_MODIFY_PATH=1     add DIR/bin to PATH in your shell's rc file
#   STRATA_DOWNLOAD_URL=URL  where releases are downloaded from (default:
#                            https://strata-lang.dev/download), for testing
#
# The release assets are named strata-<platform>.tar.gz and
# strata-<platform>.tar.gz.sha256, e.g. strata-macos-arm64.tar.gz.
#
# Everything happens in main, called on the last line, so a download cut short runs nothing.

main() {
  set -eu

  say() { printf '%s\n' "$*"; }
  err() { printf 'strata install: %s\n' "$*" >&2; exit 1; }
  need() { command -v "$1" >/dev/null 2>&1 || err "this needs \`$1\`, which isn't installed"; }

  # ── this machine ──
  need uname
  os=$(uname -s)
  arch=$(uname -m)
  case "$os/$arch" in
    Darwin/arm64 | Darwin/aarch64) platform=macos-arm64 ;;
    Darwin/x86_64)
      # a shell under Rosetta reports x86_64 on an Apple silicon Mac
      if [ "$(sysctl -n hw.optional.arm64 2>/dev/null || true)" = 1 ]; then
        platform=macos-arm64
      else
        err "Strata has no release for Intel Macs yet (only macOS on Apple silicon). It can be built from source: https://github.com/anonx3247/strata"
      fi ;;
    *) err "Strata has no release for $os $arch yet (only macOS on Apple silicon). It can be built from source: https://github.com/anonx3247/strata" ;;
  esac

  need tar
  need mktemp
  if command -v curl >/dev/null 2>&1; then
    fetch() { curl --proto '=https,http' -fsSL --retry 3 -o "$2" "$1"; }
  elif command -v wget >/dev/null 2>&1; then
    fetch() { wget -q -O "$2" "$1"; }
  else
    err "this needs \`curl\` or \`wget\`"
  fi
  if command -v shasum >/dev/null 2>&1; then
    sha256() { shasum -a 256 "$1" | cut -d ' ' -f 1; }
  elif command -v sha256sum >/dev/null 2>&1; then
    sha256() { sha256sum "$1" | cut -d ' ' -f 1; }
  else
    err "this needs \`shasum\` or \`sha256sum\` to check the download"
  fi

  version=${STRATA_VERSION:-latest}
  base=${STRATA_DOWNLOAD_URL:-https://strata-lang.dev/download}
  base=${base%/}
  dest=${STRATA_INSTALL:-$HOME/.strata}
  asset=strata-$platform.tar.gz
  url=$base/$version/$asset

  tmp=$(mktemp -d 2>/dev/null || mktemp -d -t strata)
  trap 'rm -rf "$tmp"' EXIT
  trap 'exit 1' INT TERM HUP

  # ── download and check ──
  say "Downloading Strata ($version, $platform)"
  say "  $url"
  fetch "$url" "$tmp/$asset" || err "couldn't download $url"
  fetch "$url.sha256" "$tmp/$asset.sha256" || err "couldn't download $url.sha256"
  # the file holds the hash, optionally followed by the file's name (shasum's output)
  want=$(tr -s ' \t\r' '  ' < "$tmp/$asset.sha256" | sed -n '1s/^ *\([0-9a-fA-F]*\).*/\1/p' | tr 'A-F' 'a-f')
  got=$(sha256 "$tmp/$asset")
  [ ${#want} -eq 64 ] || err "$url.sha256 doesn't hold a sha256 hash"
  [ "$want" = "$got" ] || err "the download is corrupt: its sha256 is $got, the release says $want"

  mkdir "$tmp/x"
  tar -xzf "$tmp/$asset" -C "$tmp/x" || err "couldn't unpack $asset"
  new=$tmp/x/strata
  [ -f "$new/bin/strata" ] || err "$asset has no strata/bin/strata"
  [ -d "$new/lib" ] || err "$asset has no strata/lib"

  # ── install: each top-level entry (bin, lib, …) replaced whole, so no old file lingers ──
  mkdir -p "$dest" || err "couldn't create $dest"
  for entry in "$new"/* "$new"/.[!.]*; do
    [ -e "$entry" ] || continue
    name=${entry##*/}
    rm -rf "$dest/$name.old"
    if [ -e "$dest/$name" ]; then mv "$dest/$name" "$dest/$name.old"; fi
    mv "$entry" "$dest/$name" || err "couldn't write $dest/$name"
    rm -rf "$dest/$name.old"
  done
  chmod +x "$dest/bin/strata"
  # files that came through a browser or curl may carry the quarantine flag
  if [ "$os" = Darwin ] && command -v xattr >/dev/null 2>&1; then
    xattr -dr com.apple.quarantine "$dest" 2>/dev/null || true
  fi

  say "Installed in $dest"

  # ── PATH ──
  bin=$dest/bin
  case ":${PATH:-}:" in
    *":$bin:"*) on_path=1 ;;
    *) on_path=0 ;;
  esac
  if [ "$on_path" = 0 ]; then
    shell_name=$(basename "${SHELL:-sh}")
    case "$shell_name" in
      zsh)  rc=${ZDOTDIR:-$HOME}/.zshrc; line="export PATH=\"$bin:\$PATH\"" ;;
      bash) if [ "$os" = Darwin ]; then rc=$HOME/.bash_profile; else rc=$HOME/.bashrc; fi
            line="export PATH=\"$bin:\$PATH\"" ;;
      fish) rc=$HOME/.config/fish/config.fish; line="fish_add_path \"$bin\"" ;;
      *)    rc=$HOME/.profile; line="export PATH=\"$bin:\$PATH\"" ;;
    esac
    if [ "${STRATA_MODIFY_PATH:-0}" = 1 ]; then
      if [ -f "$rc" ] && grep -qF "$line" "$rc"; then
        say "$rc already adds $bin to PATH; open a new shell to use it."
      else
        mkdir -p "$(dirname "$rc")"
        printf '\n# Strata\n%s\n' "$line" >> "$rc"
        say "Added $bin to PATH in $rc; open a new shell to use it."
      fi
    else
      say ""
      say "To use strata, add $bin to your PATH, by adding this line to $rc:"
      say ""
      say "  $line"
      say ""
      say "(or run the installer with STRATA_MODIFY_PATH=1 to have it added), then open a new shell."
    fi
  fi

  say ""
  "$dest/bin/strata" --version || err "$dest/bin/strata doesn't run"
}

main "$@"
