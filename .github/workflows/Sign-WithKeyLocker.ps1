<#
.SYNOPSIS
  Authenticode-sign the gddy.exe artifact with DigiCert KeyLocker, then
  verify the signature.

.DESCRIPTION
  Used by .github/workflows/release.yml's Windows build leg. Signing is
  gated by the caller via the `vars.ENABLE_WINDOWS_SIGNING == '1'` step
  condition; this script assumes it should sign when invoked.

  Uses a KeyLocker service-account client cert + API key and an EV Code
  Signing certificate keypair alias (both provisioned out of band). The
  DigiCert client (smctl + KSP) is installed manually from the KeyLocker
  API. `smctl sign` delegates to signtool, so the Windows SDK signtool
  directory is added to PATH.

  Adapted from gocode-client's .github/scripts/Sign-WithKeyLocker.ps1.

  Required environment variables (mapped from repo secrets by the caller):
    SM_HOST                  KeyLocker client-auth host
    SM_API_KEY               KeyLocker API token
    SM_CLIENT_CERT_PASSWORD  password for the client auth .p12
    SM_CLIENT_CERT_FILE_B64  base64 of the client auth .p12

.PARAMETER Path
  One or more paths to artifacts to sign.

.PARAMETER KeypairAlias
  KeyLocker keypair alias to sign with (repo variable KEYLOCKER_KEYPAIR_ALIAS).
#>
param(
    [Parameter(Mandatory = $true)]
    [string[]] $Path,

    [Parameter(Mandatory = $true)]
    [string] $KeypairAlias
)

$ErrorActionPreference = 'Stop'

foreach ($v in 'SM_HOST', 'SM_API_KEY', 'SM_CLIENT_CERT_PASSWORD', 'SM_CLIENT_CERT_FILE_B64') {
    if (-not (Test-Path "env:$v")) { throw "Sign-WithKeyLocker: required env var '$v' is not set." }
}

# Materialize the client authentication certificate (.p12) from the base64
# secret and point SM_CLIENT_CERT_FILE (read by smctl) at it. Cleaned up in
# the `finally` block below regardless of how the script exits.
$p12 = Join-Path $env:RUNNER_TEMP 'keylocker-client.p12'
[IO.File]::WriteAllBytes($p12, [Convert]::FromBase64String($env:SM_CLIENT_CERT_FILE_B64))
$env:SM_CLIENT_CERT_FILE = $p12

try {
    # Install the DigiCert KeyLocker client tools (smctl + KSP + PKCS#11) if
    # not already present on this runner.
    $smHome = 'C:\Program Files\DigiCert\DigiCert Keylocker Tools'
    if (-not (Test-Path (Join-Path $smHome 'smctl.exe'))) {
        $msi = Join-Path $env:RUNNER_TEMP 'Keylockertools-windows-x64.msi'
        curl.exe -fSs -X GET "$($env:SM_HOST)/signingmanager/api-ui/v1/releases/Keylockertools-windows-x64.msi/download" `
            -H "x-api-key:$($env:SM_API_KEY)" -o $msi
        if ($LASTEXITCODE -ne 0) { throw "Sign-WithKeyLocker: KeyLocker tools download failed ($LASTEXITCODE)." }
        $install = Start-Process msiexec.exe -ArgumentList "/i `"$msi`" /quiet /qn /norestart" -Wait -PassThru
        if ($install.ExitCode -ne 0) { throw "Sign-WithKeyLocker: KeyLocker tools install failed ($($install.ExitCode))." }
    }

    # `smctl sign` shells out to signtool, which is not on PATH by default.
    $signtool = (Get-ChildItem 'C:\Program Files (x86)\Windows Kits\10\bin\*\x64\signtool.exe' -ErrorAction SilentlyContinue |
        Sort-Object FullName -Descending | Select-Object -First 1)
    if (-not $signtool) { throw 'Sign-WithKeyLocker: signtool.exe not found in the Windows SDK.' }
    $env:PATH = "$smHome;$($signtool.Directory.FullName);$env:PATH"

    # Validate credentials. Redirect output to a file rather than the console:
    # `smctl healthcheck` echoes a partially-masked API key / cert password
    # that GitHub's exact-match secret masking does not catch. Removed
    # immediately on success; left in place on failure for debugging (the
    # runner itself is torn down afterward regardless).
    $healthcheckLog = "$env:RUNNER_TEMP\keylocker-healthcheck.log"
    smctl healthcheck --all *> $healthcheckLog
    if ($LASTEXITCODE -ne 0) { throw "Sign-WithKeyLocker: smctl healthcheck failed ($LASTEXITCODE) - log withheld to avoid leaking credentials." }
    Remove-Item $healthcheckLog -ErrorAction SilentlyContinue

    # Register the DigiCert KSP and sync the leaf cert into the Windows store.
    # ksp register can exit non-zero when the KSP is already registered
    # (BCryptRegisterProvider / STATUS_OBJECT_NAME_COLLISION 0xc0000035) —
    # benign, so warn rather than fail the job; certsync has no such known
    # benign-failure case, so it fails fast like the rest of the script.
    smctl windows ksp register
    if ($LASTEXITCODE -ne 0) {
        Write-Warning "smctl windows ksp register exited $LASTEXITCODE - continuing (expected if the KSP is already registered)"
    }
    smctl windows certsync --keypair-alias="$KeypairAlias"
    if ($LASTEXITCODE -ne 0) { throw "Sign-WithKeyLocker: smctl windows certsync failed ($LASTEXITCODE)." }

    foreach ($f in $Path) {
        $full = (Resolve-Path $f).Path
        Write-Host "::group::sign $full"
        smctl sign --keypair-alias "$KeypairAlias" --input $full --verbose
        if ($LASTEXITCODE -ne 0) { throw "Sign-WithKeyLocker: smctl sign failed for $full ($LASTEXITCODE)." }
        & $signtool.FullName verify /pa /v $full
        if ($LASTEXITCODE -ne 0) { throw "Sign-WithKeyLocker: signtool verify failed for $full ($LASTEXITCODE)." }
        Write-Host "::endgroup::"
    }
    Write-Host "KeyLocker signing complete for: $($Path -join ', ')"
}
finally {
    Remove-Item $p12 -ErrorAction SilentlyContinue
    Remove-Item Env:\SM_CLIENT_CERT_FILE -ErrorAction SilentlyContinue
}
