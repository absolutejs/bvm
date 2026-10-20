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

## Install

Linux, macOS and WSL:

```sh
curl -fsSL https://raw.githubusercontent.com/absolutejs/bvm/main/install.sh | sh
```

Windows (PowerShell):

```powershell
powershell -c "irm https://raw.githubusercontent.com/absolutejs/bvm/main/install.ps1 | iex"
```

npm (any OS):

```sh
npm install --global @absolutejs/bvm && bvm setup
```

The installers verify the download, put `~/.bvm/bin` first on PATH in your
shell's startup files (the user PATH and PowerShell profile on Windows), and
make `bvm` work in the terminal you installed from. If Bun is already
installed, bvm installs that same version, verified, as your default, so `bun`
keeps meaning what it meant. Output is colored on a terminal; set `NO_COLOR`
to turn it off.

## Uninstall

```sh
bvm uninstall 1.3.9      # remove one Bun version
bvm self uninstall       # remove bvm itself
```

`bvm self uninstall` keeps one Bun, where Bun's own installer puts it
(`~/.bun/bin`), so `bun` and `bun upgrade` keep working. On a terminal you pick
it with the arrow keys (your default is preselected) and confirm the plan;
in scripts, pass `--keep <version>`, `--keep-default` or `--remove-bun`, plus
`--yes`. It removes everything bvm added to your shell startup files (and,
on Windows, your user PATH and PowerShell profile) and leaves the rest of
those files as they were.

MIT licensed.

## Patched build identity

New AbsoluteJS builds use build metadata, for example `1.4.3+absolute.2`.
BVM accepts these alongside existing `1.4.2-absolute.1` releases, preserving
each release's exact tag and installation directory. Both satisfy engine
ranges according to their upstream Bun version.

`bun -v` and `Bun.version` come from the executable, unchanged by BVM. New
patched binaries embed their AbsoluteJS label; older binaries still report
only the upstream version. `bvm current` shows the selected release and why;
`bvm which` shows its binary. Inside an application, `process.execPath`
identifies the executable actually running it. Updating BVM does not relabel
or replace an already installed Bun.
