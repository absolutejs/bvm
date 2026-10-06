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

die() { echo "bvm install: $*" >&2; exit 1; }

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

printf '%s\n' "$PUBLIC_KEY" > "$work/key.pem"
base64 -d < "$work/SHASUMS256.txt.sig" > "$work/sig.bin" 2>/dev/null || base64 -D < "$work/SHASUMS256.txt.sig" > "$work/sig.bin"
if openssl pkeyutl -verify -pubin -inkey "$work/key.pem" -rawin -in "$work/SHASUMS256.txt" -sigfile "$work/sig.bin" >/dev/null 2>&1; then
  echo "bvm install: signature verified (AbsoluteJS release key)"
elif openssl version 2>/dev/null | grep -q '^OpenSSL 3'; then
  die "SHASUMS256.txt is not signed by the AbsoluteJS release key"
else
  echo "bvm install: this OpenSSL cannot check Ed25519 signatures; verified the checksum over HTTPS only"
fi

mkdir -p "$BVM_DIR/bin"
chmod +x "$work/$asset"
mv "$work/$asset" "$BVM_DIR/bin/bvm"
BVM_DIR="$BVM_DIR" "$BVM_DIR/bin/bvm" setup
echo "bvm install: done. Open a new terminal, then: bvm install latest --default"
