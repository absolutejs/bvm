#!/usr/bin/env bash
# End-to-end: real downloads, real signature checks, real shims, on whatever
# OS this runs on (Linux, macOS, or Windows under Git Bash).
#   tests/e2e/run.sh <path to the built bvm binary>
set -euo pipefail
BVM="$(cd "$(dirname "$1")" && pwd)/$(basename "$1")"
WORK="$(mktemp -d)"
export BVM_DIR="$WORK/bvm"
export HOME="$WORK/home" USERPROFILE="$WORK/home"
mkdir -p "$HOME"
touch "$HOME/.bashrc"
EXE=""; case "$(uname -s)" in MINGW*|MSYS*|CYGWIN*) EXE=".exe" ;; esac
fail() { echo "FAIL: $*" >&2; exit 1; }
pass() { echo "ok - $*"; }

expected="bvm $(sed -n 's/^version = "\(.*\)"/\1/p' "$(dirname "$0")/../../Cargo.toml" | head -1)"
for flag in -v -V --version; do
  [ "$("$BVM" "$flag")" = "$expected" ] || fail "bvm $flag does not print the version"
done
pass "bvm -v, -V and --version print the version"

"$BVM" install 1.4.2 --default
[ "$("$("$BVM" which 1.4.2)" --version)" = "1.4.2" ] || fail "official 1.4.2 did not install"
pass "official Bun 1.4.2 installs after Bun's PGP signature verifies"

"$BVM" install 1.4.2-absolute.1
[ "$("$("$BVM" which 1.4.2-absolute.1)" --version)" = "1.4.2" ] || fail "AbsoluteJS build did not install"
pass "the AbsoluteJS build installs after its Ed25519 signature verifies"

env -u GITHUB_TOKEN "$BVM" install latest || fail "install latest without GITHUB_TOKEN"
pass "latest resolves without the GitHub API (no token, no rate limit)"

"$BVM" setup >/dev/null
export PATH="$BVM_DIR/bin:$PATH"
[ -x "$BVM_DIR/bin/bun$EXE" ] || fail "no bun shim"
pass "setup installs the shims"

PROBE='const t=new Bun.Transpiler({loader:"tsx",reactFastRefresh:true});console.log(t.transformSync("export function A(){return <b/>}").includes("$RefreshReg$")?"patched":"stock")'
mkdir -p "$WORK/pinned/deep" "$WORK/engines"
echo "1.4.2-absolute.1" > "$WORK/pinned/.bun-version"
echo '{"engines":{"bun":">=1.4.0"}}' > "$WORK/engines/package.json"
[ "$(cd "$WORK/pinned/deep" && bun -e "$PROBE")" = "patched" ] || fail ".bun-version did not select the AbsoluteJS build"
pass ".bun-version in a parent directory selects the pinned build"
[ "$(cd "$WORK/engines" && bun -e "$PROBE")" = "stock" ] || fail "engines.bun did not select the official default"
pass "engines.bun selects an installed version that satisfies it"
[ "$(cd "$WORK/engines" && BVM_BUN_VERSION=1.4.2-absolute.1 bun -e "$PROBE")" = "patched" ] || fail "BVM_BUN_VERSION was ignored"
pass "BVM_BUN_VERSION overrides the directory"
[ "$(cd "$WORK/engines" && bunx --version)" = "1.4.2" ] || fail "bunx"
pass "bunx runs through the selected Bun"
set +e; (cd "$WORK/engines" && bun -e 'process.exit(7)'); code=$?; set -e
[ "$code" = "7" ] || fail "exit code was $code, not 7"
pass "the shim passes Bun's exit code through"

# A checksum list that does not verify must stop the install before anything
# is extracted. Serve the real 1.4.2 list with one digit changed.
set +e; out=$(BVM_TEST_TAMPER=1 "$BVM" install 1.4.1 2>&1); code=$?; set -e
if [ "$code" = "0" ]; then fail "a tampered checksum list was accepted"; fi
case "$out" in *"not signed by Bun's release key"*) ;; *) fail "unexpected refusal: $out" ;; esac
[ ! -e "$BVM_DIR/versions/1.4.1" ] || fail "something was installed from a tampered list"
pass "a tampered checksum list is refused and nothing is installed"

echo "all end-to-end checks passed"
