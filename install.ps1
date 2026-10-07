# Installs bvm, the Bun version manager, on Windows.
#
#   powershell -c "irm https://raw.githubusercontent.com/absolutejs/bvm/main/install.ps1 | iex"
#
# Downloads the latest release for this machine and checks it against the
# release's SHASUMS256.txt. (Windows PowerShell has no Ed25519, so the list's
# signature is not checked here; from then on bvm verifies every Bun it
# installs, and its own updates, against keys compiled into it.) Set BVM_DIR to
# install somewhere other than %USERPROFILE%\.bvm.
$ErrorActionPreference = 'Stop'
$repo = 'absolutejs/bvm'
$bvmDir = if ($env:BVM_DIR) { $env:BVM_DIR } else { Join-Path $env:USERPROFILE '.bvm' }
$arch = switch ($env:PROCESSOR_ARCHITECTURE) {
  'AMD64' { 'x64' }
  'ARM64' { 'arm64' }
  default { throw "bvm: unsupported architecture $($env:PROCESSOR_ARCHITECTURE)" }
}
$asset = "bvm-windows-$arch.exe"
$base = "https://github.com/$repo/releases/latest/download"
$work = Join-Path ([System.IO.Path]::GetTempPath()) ("bvm-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $work | Out-Null
try {
  Invoke-WebRequest -UseBasicParsing "$base/$asset" -OutFile (Join-Path $work $asset)
  Invoke-WebRequest -UseBasicParsing "$base/SHASUMS256.txt" -OutFile (Join-Path $work 'SHASUMS256.txt')
  $line = Get-Content (Join-Path $work 'SHASUMS256.txt') | Where-Object { ($_ -split '\s+')[1] -eq $asset } | Select-Object -First 1
  if (-not $line) { throw "bvm: $asset is not in SHASUMS256.txt" }
  $expected = ($line -split '\s+')[0].ToLower()
  $actual = (Get-FileHash -Algorithm SHA256 (Join-Path $work $asset)).Hash.ToLower()
  if ($actual -ne $expected) { throw "bvm: $asset does not match its checksum" }
  $bin = Join-Path $bvmDir 'bin'
  New-Item -ItemType Directory -Force -Path $bin | Out-Null
  $target = Join-Path $bin 'bvm.exe'
  if (Test-Path $target) { Rename-Item $target ("bvm.exe.old-" + [DateTimeOffset]::Now.ToUnixTimeMilliseconds()) }
  Move-Item (Join-Path $work $asset) $target
  $env:BVM_DIR = $bvmDir
  # The profile this PowerShell actually loads: it follows a OneDrive-redirected
  # Documents folder and differs between PowerShell 7 and Windows PowerShell 5.1.
  $env:BVM_POWERSHELL_PROFILE = $PROFILE.CurrentUserCurrentHost
  & $target setup
  Remove-Item Env:\BVM_POWERSHELL_PROFILE -ErrorAction SilentlyContinue
  # `irm ... | iex` runs in this session, so bvm can be live here right away:
  # PATH (new windows read it from the user environment) and the `bvm use`
  # function.
  if (-not ($env:Path -split ';' -contains $bin)) { $env:Path = "$bin;$env:Path" }
  Invoke-Expression (& $target env --shell powershell | Out-String)
  Write-Host ''
  Write-Host "bvm: installed $(& $target --version) to $bvmDir. It works in this window and new ones."
  Write-Host '  Next: bvm install latest --default'
}
finally {
  Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
}
