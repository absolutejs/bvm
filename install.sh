#!/bin/sh
# Installs bvm, the Bun version manager, on Linux and macOS.
#
#   curl -fsSL https://raw.githubusercontent.com/absolutejs/bvm/main/install.sh | sh
#
# Downloads the latest release for this machine, checks it against the
# release's SHASUMS256.txt, and checks that list's Ed25519 signature by the
# AbsoluteJS release key when this system's OpenSSL can (OpenSSL 3; macOS's
# LibreSSL cannot, so there it relies on HTTPS for the list). From then on bvm
# verifies every Bun it installs, and its own updates, against keys compiled
# into it. Set BVM_DIR to install somewhere other than ~/.bvm.
set -eu

REPO="absolutejs/bvm"
BVM_DIR="${BVM_DIR:-$HOME/.bvm}"
PUBLIC_KEY='-----BEGIN PUBLIC KEY-----
MCowBQYDK2VwAyEAQl0pUETGfqE4xLc4YcFrN/Yu1aZDVbDjkkF5HSTn7p0=
-----END PUBLIC KEY-----'

# Color and symbols on a terminal; plain `bvm install:` lines in pipes and
# logs, or with NO_COLOR set.
if { [ -t 1 ] && [ -z "${NO_COLOR:-}" ] && [ "${TERM:-}" != dumb ]; } || [ -n "${CLICOLOR_FORCE:-}" ]; then
  bold=$(printf '\033[1m') dim=$(printf '\033[2m') green=$(printf '\033[1;32m')
  cyan=$(printf '\033[36m') red=$(printf '\033[1;31m') reset=$(printf '\033[0m')
  ok() { printf '  %s✓%s %s\n' "$green" "$reset" "$*"; }
  die() { printf '%serror:%s %s\n' "$red" "$reset" "$*" >&2; exit 1; }
  color=1
else
  bold='' dim='' green='' cyan='' red='' reset=''
  ok() { echo "bvm install: $*"; }
  die() { echo "bvm install: $*" >&2; exit 1; }
  color=''
fi

case "$(uname -s)" in
  Linux) os=linux ;;
  Darwin) os=darwin ;;
  *) die "unsupported system $(uname -s); on Windows use install.ps1" ;;
esac
case "$(uname -m)" in
  x86_64 | amd64) arch=x64 ;;
  arm64 | aarch64) arch=arm64 ;;
  *) die "unsupported architecture $(uname -m)" ;;
esac
asset="bvm-$os-$arch"
echo
echo "  ${bold}Installing bvm$reset ${dim}(the Bun version manager, $os-$arch)$reset"
echo
base="https://github.com/$REPO/releases/latest/download"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT INT TERM
fetch() { curl -fsSL --proto '=https' --tlsv1.2 -o "$work/$1" "$base/$1" || die "could not download $1"; }
fetch "$asset"
fetch SHASUMS256.txt
fetch SHASUMS256.txt.sig

if command -v sha256sum >/dev/null 2>&1; then
  actual=$(sha256sum "$work/$asset" | cut -d' ' -f1)
else
  actual=$(shasum -a 256 "$work/$asset" | cut -d' ' -f1)
fi
expected=$(awk -v name="$asset" '$2 == name || $2 == "*" name { print $1 }' "$work/SHASUMS256.txt")
[ -n "$expected" ] || die "$asset is not in SHASUMS256.txt"
[ "$actual" = "$expected" ] || die "$asset does not match its checksum"
ok "Downloaded $bold$asset$reset; its checksum matches"

printf '%s\n' "$PUBLIC_KEY" > "$work/key.pem"
base64 -d < "$work/SHASUMS256.txt.sig" > "$work/sig.bin" 2>/dev/null || base64 -D < "$work/SHASUMS256.txt.sig" > "$work/sig.bin"
if openssl pkeyutl -verify -pubin -inkey "$work/key.pem" -rawin -in "$work/SHASUMS256.txt" -sigfile "$work/sig.bin" >/dev/null 2>&1; then
  ok "Signature verified (AbsoluteJS release key)"
elif openssl version 2>/dev/null | grep -q '^OpenSSL 3'; then
  die "SHASUMS256.txt is not signed by the AbsoluteJS release key"
else
  ok "Checksum verified over HTTPS ${dim}(this OpenSSL cannot check Ed25519 signatures)$reset"
fi

mkdir -p "$BVM_DIR/bin"
chmod +x "$work/$asset"
mv "$work/$asset" "$BVM_DIR/bin/bvm"
if [ -n "$color" ]; then tone=CLICOLOR_FORCE; else tone=NO_COLOR; fi
env "$tone=1" BVM_FROM_INSTALLER=1 BVM_DIR="$BVM_DIR" "$BVM_DIR/bin/bvm" setup 2>&1

# A script cannot change the PATH of the shell that ran it, so `bvm` would
# only exist in new terminals. When a directory already on this shell's PATH
# is ours to write (~/.local/bin on most Linux setups, even before it exists),
# link bvm into it so it works right away.
linked=""
for dir in "$HOME/.local/bin" "$HOME/bin"; do
  case ":$PATH:" in
    *":$dir:"*)
      # On PATH but not created yet (a fresh account): ours to create.
      [ -d "$dir" ] || mkdir -p "$dir" 2>/dev/null || continue
      if [ -w "$dir" ]; then
        ln -sf "$BVM_DIR/bin/bvm" "$dir/bvm"
        linked="$dir/bvm"
        ok "Linked bvm into $bold$(echo "$dir" | sed "s|^$HOME|~|")$reset so it works in this terminal"
        break
      fi
      ;;
  esac
done

# Shown with $HOME rather than the expanded path, which is long and the same
# for everyone.
bvm_path=$(echo "$BVM_DIR/bin/bvm" | sed "s|^$HOME/|\$HOME/|")
case "$(basename "${SHELL:-sh}")" in
  fish) activate="\"$bvm_path\" env --shell fish | source" ;;
  *) activate="eval \"\$(\"$bvm_path\" env)\"" ;;
esac

version=$("$BVM_DIR/bin/bvm" --version | cut -d' ' -f2)
where=$(echo "$BVM_DIR" | sed "s|^$HOME|~|")
echo
echo "  ${green}bvm $version$reset is installed in $bold$where$reset"
echo
if [ -n "$linked" ]; then
  echo "  bvm works in this terminal now. New terminals also switch bun and bunx"
  echo "  per project; to get that here too, run:"
else
  echo "  New terminals are set up. To use bvm in this terminal now, run:"
fi
echo
echo "    $cyan$activate$reset"
echo
if [ -z "$("$BVM_DIR/bin/bvm" ls 2>/dev/null)" ]; then
  echo "  Then install Bun: $cyan""bvm install latest --default$reset"
  echo
fi
