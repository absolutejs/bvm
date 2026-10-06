# bvm — the Bun version manager

`@absolutejs/bvm` installs, switches and verifies Bun versions, the way nvm does
for Node, on Linux, macOS and Windows from day one. It also serves AbsoluteJS's
own patched builds as an official, signed channel, so a patched Bun can never be
mistaken for (or replaced by) a fake binary.

## Why our own

Bun has no built-in switching: `bun upgrade` only installs the latest release or
canary. Third-party managers exist (bum, several unrelated "bvm"s, proto, mise),
but none verifies a second publisher's binaries, and AbsoluteJS ships one
(`absolutejs/patched-bun`). Today that patched build is installed by the
AbsoluteJS CLI (`absolute bun-patch`) and pinned by hand in PAAS images; bvm
replaces both.

## Language: Rust

bvm cannot depend on the runtime it manages, so it is a native binary. Rust
matches Bun itself and the version-manager field (fnm, Volta, proto, mise, bum).
Cross-compilation uses the sysroots `patched-bun/scripts/setup-cross.sh`
installed in `/opt` (macOS SDK, Windows SDK, glibc/musl) with clang/lld, as Bun
links its own Rust. Behavior on macOS and Windows is tested natively in CI
(shims, PATH and Windows links are where version managers break).

## Commands

```
bvm install <version|latest|channel>   # 1.4.2, latest, 1.4.2-absolute.1, absolute
bvm uninstall <version>
bvm use <version>                      # this shell
bvm default <version>                  # everywhere else
bvm ls                                 # installed
bvm ls-remote [--absolute]             # available
bvm current | which | exec <version> -- <cmd...>
bvm setup                              # put the shims on PATH (shell rc / HKCU PATH)
bvm self update
```

## Which Bun runs

The shims (`bun`, `bunx`) are bvm itself, dispatched on argv[0]. For each call
they pick, in order: `BVM_BUN_VERSION` (set by `bvm use`), the nearest
`.bun-version` walking up from the working directory, the nearest
`package.json` `engines.bun` (highest installed version that satisfies it), then
the default. Then they exec the real binary (execve on Unix; a child with
forwarded exit code and Ctrl-C on Windows).

## Trust

Nothing runs unverified.

| Channel | Source | Verified by |
| --- | --- | --- |
| official | `oven-sh/bun` releases | Bun's clearsigned `SHASUMS256.txt.asc`, PGP key `F3DCC08A8572C0749B3E18888EAB4D40A7B22B59` (Robobun, pinned in Bun's own Docker images) embedded in bvm; then the zip's SHA-256 against that signed list |
| absolute | `absolutejs/patched-bun` releases | an Ed25519 signature over `SHASUMS256.txt` by the AbsoluteJS release key, whose public half is embedded in bvm; then the zip's SHA-256 |

A download whose signature does not verify is deleted and never extracted. The
AbsoluteJS private key never lives in CI or on build machines: patched-bun's
`release.sh` signs on the release machine (key at
`~/.local/share/absolutejs/keys/`, back it up offline). bvm embeds a list of
accepted keys so the key can be rotated without breaking older releases. bvm's
own releases are signed with the same key, and `bvm self update` checks them.

## Layout

```
~/.bvm/                       (%USERPROFILE%\.bvm on Windows; BVM_DIR overrides)
  bin/bun, bin/bunx, bin/bvm  shims (hard links/copies of bvm)
  versions/1.4.2/bun          official
  versions/1.4.2-absolute.1/bun
  default                     the default version
  cache/                      verified downloads
```

## Milestones

1. **Core:** install/uninstall/ls/ls-remote/use/default/current/which/exec,
   both channels with full verification, shims and version resolution, `setup`,
   on all three OSes, with native CI on each.
2. **Distribution:** signed GitHub releases of bvm, `install.sh` /
   `install.ps1`, the npm package `@absolutejs/bvm` (per-platform binaries as
   optional dependencies, like esbuild), `bvm self update`.
3. **Adoption:** patched-bun's `release.sh` signs releases; the AbsoluteJS CLI's
   `absolute bun-patch` delegates to bvm; PAAS images install Bun through bvm.

## License

MIT: a single-purpose tool in a commodity category (Tier B under the AbsoluteJS
licensing policy), with no hosted-product story to protect.
