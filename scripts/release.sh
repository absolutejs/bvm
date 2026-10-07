#!/usr/bin/env bash
# Publishes a bvm release. CI (release.yml) builds every platform's binary into
# a draft for the tag; this script, on the release machine, checks them, signs
# SHASUMS256.txt with the AbsoluteJS release key and only then publishes. The
# key never leaves this machine.
#
#   scripts/release.sh          # releases the version in Cargo.toml
set -euo pipefail
cd "$(dirname "$0")/.."
REPO=absolutejs/bvm
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
TAG="v$VERSION"
KEY="${ABSOLUTEJS_RELEASE_KEY:-$HOME/.local/share/absolutejs/keys/bvm-release-ed25519.pem}"
EXPECTED="bvm-linux-x64 bvm-linux-arm64 bvm-darwin-arm64 bvm-darwin-x64 bvm-windows-x64.exe bvm-windows-arm64.exe"
die() { echo "release: $*" >&2; exit 1; }

[ -r "$KEY" ] || die "no release signing key at $KEY"
[ -z "$(git status --porcelain)" ] || die "the working tree is not clean"
[ "$(git branch --show-current)" = main ] || die "release from main"
git fetch -q origin
[ "$(git rev-parse HEAD)" = "$(git rev-parse origin/main)" ] || die "main is not pushed (or is behind origin)"
if gh release view "$TAG" -R "$REPO" --json isDraft -q .isDraft 2>/dev/null | grep -q false; then
  die "$TAG is already published"
fi

if ! git rev-parse -q --verify "refs/tags/$TAG" >/dev/null; then
  git tag -a "$TAG" -m "bvm $VERSION"
  git push -q origin "$TAG"
  echo "release: tagged $TAG; waiting for the release build"
fi

# Wait for the build of exactly this tag to finish.
sha=$(git rev-list -n1 "$TAG")
run=""
for _ in $(seq 1 60); do
  run=$(gh run list -R "$REPO" --workflow release.yml --json databaseId,headSha -q "[.[] | select(.headSha==\"$sha\")][0].databaseId // empty")
  [ -n "$run" ] && break
  sleep 10
done
[ -n "$run" ] || die "no release build started for $TAG"
gh run watch "$run" -R "$REPO" --exit-status >/dev/null || die "the release build failed: gh run view $run -R $REPO"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
gh release download "$TAG" -R "$REPO" --dir "$work" --pattern 'bvm-*'
for name in $EXPECTED; do [ -s "$work/$name" ] || die "the draft is missing $name"; done
[ "$(ls "$work" | wc -l)" = 6 ] || die "the draft has unexpected assets: $(ls "$work")"

# Each binary is the format and architecture its name claims.
check() { file -b "$work/$1" | grep -qE "$2" || die "$1 is not $2: $(file -b "$work/$1")"; }
check bvm-linux-x64 'ELF 64-bit.*x86-64.*static'
check bvm-linux-arm64 'ELF 64-bit.*aarch64.*static'
check bvm-darwin-arm64 'Mach-O 64-bit.*arm64'
check bvm-darwin-x64 'Mach-O 64-bit.*x86_64'
check bvm-windows-x64.exe 'PE32\+.*x86-64'
check bvm-windows-arm64.exe 'PE32\+.*Aarch64'
chmod +x "$work/bvm-linux-x64"
got=$("$work/bvm-linux-x64" --version)
[ "$got" = "bvm $VERSION" ] || die "bvm-linux-x64 reports '$got', not 'bvm $VERSION'"

(cd "$work" && sha256sum $EXPECTED > SHASUMS256.txt)
openssl pkeyutl -sign -inkey "$KEY" -rawin -in "$work/SHASUMS256.txt" -out "$work/sig.bin"
openssl pkey -in "$KEY" -pubout | openssl pkeyutl -verify -pubin -inkey /dev/stdin -rawin \
  -in "$work/SHASUMS256.txt" -sigfile "$work/sig.bin" >/dev/null || die "signature did not verify"
base64 -w0 "$work/sig.bin" > "$work/SHASUMS256.txt.sig" && echo >> "$work/SHASUMS256.txt.sig"

# npm: one package per platform holding exactly the binary just signed, then
# @absolutejs/bvm, whose launcher runs whichever one npm installed.
npm_dir="$work/npm"
optional=""
npm_names=""
# Whether the registry serves this exact version (so a rerun skips it).
published() { [ "$(npm view "$1@$VERSION" version 2>/dev/null)" = "$VERSION" ]; }
for asset in $EXPECTED; do
  stem=${asset%.exe}; platform=${stem#bvm-}; os=${platform%-*}; cpu=${platform#*-}
  npm_os=$os; [ "$os" = windows ] && npm_os=win32
  name="@absolutejs/bvm-$platform"
  dir="$npm_dir/$platform"
  mkdir -p "$dir/bin"
  binary=bvm; [ "$os" = windows ] && binary=bvm.exe
  cp "$work/$asset" "$dir/bin/$binary"
  chmod +x "$dir/bin/$binary"
  cat > "$dir/package.json" <<JSON
{
  "name": "$name",
  "version": "$VERSION",
  "description": "bvm for $platform (see @absolutejs/bvm)",
  "license": "MIT",
  "repository": { "type": "git", "url": "git+https://github.com/absolutejs/bvm.git" },
  "os": ["$npm_os"],
  "cpu": ["$cpu"],
  "files": ["bin"]
}
JSON
  published "$name" || (cd "$dir" && npm publish --access public)
  optional="$optional\"$name\": \"$VERSION\","
  npm_names="$npm_names $name"
done
cp -R npm/bvm "$npm_dir/bvm"
cp README.md LICENSE "$npm_dir/bvm/"
node -e '
const fs = require("node:fs");
const file = process.argv[1];
const pkg = JSON.parse(fs.readFileSync(file, "utf8"));
pkg.version = process.argv[2];
pkg.optionalDependencies = JSON.parse("{" + process.argv[3].replace(/,$/, "") + "}");
pkg.files = ["bin", "README.md", "LICENSE"];
fs.writeFileSync(file, JSON.stringify(pkg, null, "\t") + "\n");
' "$npm_dir/bvm/package.json" "$VERSION" "$optional"
published @absolutejs/bvm || (cd "$npm_dir/bvm" && npm publish --access public)

# Publishing the GitHub release starts install-check, which installs from npm:
# wait until the registry serves all seven packages (new versions can take
# several minutes to appear). The version shows in the metadata before its
# tarball downloads (npm install then 404s), so wait for the tarball too.
tarball_served() {
  url=$(npm view "$1@$VERSION" dist.tarball 2>/dev/null)
  [ -n "$url" ] && curl -fsI "$url" >/dev/null 2>&1
}
for _ in $(seq 1 90); do
  missing=""
  for name in $npm_names @absolutejs/bvm; do
    { published "$name" && tarball_served "$name"; } || missing="$missing $name"
  done
  [ -z "$missing" ] && break
  sleep 20
done
[ -z "$missing" ] || die "npm is not serving$missing@$VERSION yet; rerun this script once it is"
echo "release: npm serves @absolutejs/bvm@$VERSION and its six platform packages"

notes="$work/notes.md"
{
  echo "bvm $VERSION"
  echo
  echo "Every binary is listed in \`SHASUMS256.txt\`, signed with the AbsoluteJS release key (\`SHASUMS256.txt.sig\`, Ed25519)."
  echo "\`bvm self update\` and the install scripts check that signature before running anything."
} > "$notes"
gh release upload "$TAG" -R "$REPO" --clobber "$work/SHASUMS256.txt" "$work/SHASUMS256.txt.sig"
gh release edit "$TAG" -R "$REPO" --draft=false --latest --notes-file "$notes"
echo "release: published https://github.com/$REPO/releases/tag/$TAG"

