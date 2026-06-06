<#
.SYNOPSIS
    gddy installer (Rust-port alpha) for Windows / PowerShell.

.DESCRIPTION
    Downloads, verifies, and installs the GoDaddy CLI Rust-port ALPHA binary
    (gddy.exe) from the rolling `alpha` GitHub prerelease.

    `gddy` installs alongside the legacy `godaddy` CLI so you can run both. It is
    an experimental build that tracks the tip of `rust-port`; expect breakage.

    macOS/Linux (and Git Bash/MSYS2/Cygwin) users should use install.sh instead.

.PARAMETER Prefix
    Install directory. Defaults to "$env:LOCALAPPDATA\Programs\gddy".

.EXAMPLE
    irm https://github.com/godaddy/cli/releases/download/alpha/install.ps1 | iex

.EXAMPLE
    .\install.ps1 -Prefix "C:\tools\gddy"
#>
[CmdletBinding()]
param(
    [string]$Prefix
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$Repo = 'godaddy/cli'
$Tag  = 'alpha'

function Write-Info  { param([string]$Message) Write-Host "==> $Message" -ForegroundColor Blue }
function Write-Warn  { param([string]$Message) Write-Host "WARN: $Message" -ForegroundColor Yellow }
# Use `throw`, not `exit`: the documented invocation is `irm ... | iex`, which
# runs this script in the caller's session — `exit` would close their terminal.
function Die         { param([string]$Message) Write-Host "ERROR: $Message" -ForegroundColor Red; throw $Message }

# Resolve the default install dir here (not in the param default) so a missing
# LOCALAPPDATA can fall back / error cleanly instead of throwing in Join-Path
# before Die is available.
if (-not $Prefix) {
    $base = if ($env:LOCALAPPDATA)   { $env:LOCALAPPDATA }
            elseif ($env:USERPROFILE) { $env:USERPROFILE }
            else { Die "Cannot determine a default install directory (LOCALAPPDATA and USERPROFILE are both unset). Re-run with -Prefix <dir>." }
    $Prefix = Join-Path $base 'Programs\gddy'
}

# Force TLS 1.2 for older PowerShell / .NET defaults.
try { [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocol]::Tls12 } catch { }

# ── 1. Detect platform ───────────────────────────────────────────────────────

$arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString().ToLower()
switch ($arch) {
    'x64'   { $platform = 'x86_64-pc-windows-msvc' }
    default {
        Die ("Unsupported architecture '$arch'. Only x86_64-pc-windows-msvc is published today. " +
             "Download the matching asset manually from https://github.com/$Repo/releases/tag/$Tag")
    }
}

$bin       = 'gddy.exe'
$archive   = "gddy-$platform.zip"
$checksums = 'gddy-checksums-sha256.txt'
$baseUrl   = "https://github.com/$Repo/releases/download/$Tag"

Write-Info "Detected platform: $platform"
Write-Info "Installing gddy alpha ($Tag)"

# ── 2. Download + verify ─────────────────────────────────────────────────────

$work = Join-Path ([System.IO.Path]::GetTempPath()) ("gddy-" + [System.Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $work -Force | Out-Null
try {
    $archivePath   = Join-Path $work $archive
    $checksumsPath = Join-Path $work $checksums

    Write-Info "Downloading $archive and checksums..."
    try {
        Invoke-WebRequest -Uri "$baseUrl/$archive"   -OutFile $archivePath   -UseBasicParsing
    } catch {
        Die "Failed to download $archive. Has the $Tag release been published yet?"
    }
    try {
        Invoke-WebRequest -Uri "$baseUrl/$checksums" -OutFile $checksumsPath -UseBasicParsing
    } catch {
        Die "Failed to download $checksums."
    }

    Write-Info "Verifying checksum..."
    # Each line is "<sha256>  <filename>"; match the filename exactly.
    $expected = $null
    foreach ($line in Get-Content $checksumsPath) {
        $parts = $line -split '\s+', 2
        if ($parts.Count -eq 2 -and $parts[1].Trim() -eq $archive) {
            $expected = $parts[0].Trim().ToLower()
            break
        }
    }
    if (-not $expected) { Die "Checksum for $archive not found in $checksums." }

    $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $archivePath).Hash.ToLower()
    if ($expected -ne $actual) {
        Die "Checksum mismatch!`n  Expected: $expected`n  Got:      $actual"
    }
    Write-Info "Checksum verified."

    # ── 3. Extract + install ─────────────────────────────────────────────────

    $extractDir = Join-Path $work 'extract'
    New-Item -ItemType Directory -Path $extractDir -Force | Out-Null
    Expand-Archive -LiteralPath $archivePath -DestinationPath $extractDir -Force

    $extractedBin = Join-Path $extractDir $bin
    if (-not (Test-Path -LiteralPath $extractedBin)) {
        Die "Unexpected archive layout — expected a '$bin' binary at the archive root."
    }

    New-Item -ItemType Directory -Path $Prefix -Force | Out-Null
    $destBin = Join-Path $Prefix $bin
    Copy-Item -LiteralPath $extractedBin -Destination $destBin -Force
    Write-Info "Binary installed to $destBin"

    # ── 4. PATH ──────────────────────────────────────────────────────────────

    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    $onPath = ($userPath -split ';' | Where-Object { $_.TrimEnd('\') -ieq $Prefix.TrimEnd('\') }).Count -gt 0
    if (-not $onPath) {
        $newPath = if ([string]::IsNullOrEmpty($userPath)) { $Prefix } else { "$userPath;$Prefix" }
        [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
        $env:Path = "$env:Path;$Prefix"  # current session
        Write-Info "Added $Prefix to your user PATH (restart your shell for new shells to see it)."
    }
} finally {
    Remove-Item -Recurse -Force -LiteralPath $work -ErrorAction SilentlyContinue
}

# ── 5. Success ────────────────────────────────────────────────────────────────

Write-Host ""
Write-Info "gddy alpha ($Tag) installed successfully!"
Write-Host ""
Write-Host "  Binary:    $(Join-Path $Prefix $bin)"
Write-Host "  Verify:    gddy --version"
Write-Host ""
Write-Host "  Note: this is an experimental Rust-port alpha that installs alongside"
Write-Host "  the current 'godaddy' CLI."
Write-Host ""
Write-Host "Share this one-liner with your team:"
Write-Host ""
Write-Host "  irm https://github.com/$Repo/releases/download/$Tag/install.ps1 | iex"
Write-Host ""
