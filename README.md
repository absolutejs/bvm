# bvm

Install, switch and verify [Bun](https://bun.com) versions, the way nvm does
for Node, on Linux, macOS and Windows.

```sh
bvm install latest --default     # official Bun
bvm install 1.4.2-absolute.1     # AbsoluteJS's patched build
bvm use 1.4.2                    # this shell
echo 1.4.2-absolute.1 > .bun-version   # this project
bvm ls
```

`bun` and `bunx` are shims: each call runs the version selected by
`BVM_BUN_VERSION` (set by `bvm use`), the nearest `.bun-version`, the nearest
`package.json` `engines.bun`, or the default, in that order. A version a project
pins exactly is installed on first use.

## Nothing runs unverified

Official Bun is checked against Bun's own signed checksum list
(`SHASUMS256.txt.asc`, PGP key `F3DC C08A 8572 C074 9B3E 1888 8EAB 4D40 A7B2 2B59`,
the key Bun's Docker images pin). AbsoluteJS builds are checked against an
Ed25519 signature by the AbsoluteJS release key. Both keys are compiled into
bvm, so a mirror or a modified download cannot pass as either.

See [PLAN.md](PLAN.md) for the design.

## Setup

```sh
bvm setup    # installs the shims in ~/.bvm/bin and adds them to your shell
```

MIT licensed.
