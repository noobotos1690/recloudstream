<#
.SYNOPSIS
    Build, stage and package CloudStream Desktop.

.DESCRIPTION
    One command, in order:
      1. read the version from crates/app/Cargo.toml
      2. cargo build --release for the MSVC target
      3. stage dist\ (exe, optional libmpv-2.dll, licence files)
      4. verify the exe's imports (fails on an unexpected dependency)
      5. compile the installer with Inno Setup 6
      6. optionally sign

.PARAMETER Version
    Override the version written into the installer and file metadata.

.PARAMETER SkipInstaller
    Build and stage only; useful on machines without Inno Setup.

.PARAMETER Sign
    Sign the exe and installer with signtool (requires a certificate in the
    current user's store).

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File scripts\build.ps1
#>

[CmdletBinding()]
param(
    [string]$Version,
    [switch]$SkipInstaller,
    [switch]$Sign
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Push-Location $root
try {
    # -- 1. version ---------------------------------------------------------
    if (-not $Version) {
        $manifest = Get-Content 'crates\app\Cargo.toml' -Raw
        if ($manifest -match 'version\.workspace\s*=\s*true') {
            $workspace = Get-Content 'Cargo.toml' -Raw
            if ($workspace -notmatch '(?m)^version\s*=\s*"([^"]+)"') {
                throw 'Could not read the workspace version from Cargo.toml'
            }
            $Version = $Matches[1]
        }
        elseif ($manifest -match '(?m)^version\s*=\s*"([^"]+)"') {
            $Version = $Matches[1]
        }
        else {
            throw 'Could not determine the application version'
        }
    }
    Write-Host "Building CloudStream Desktop $Version" -ForegroundColor Cyan

    # -- 2. compile --------------------------------------------------------
    cargo build --release --locked --target x86_64-pc-windows-msvc
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed with exit code $LASTEXITCODE" }

    $exe = 'target\x86_64-pc-windows-msvc\release\cloudstream-desktop.exe'
    if (-not (Test-Path $exe)) { throw "Expected binary not found: $exe" }

    # -- 3. stage ----------------------------------------------------------
    if (Test-Path 'dist') { Remove-Item 'dist' -Recurse -Force }
    New-Item -ItemType Directory -Path 'dist' | Out-Null
    Copy-Item $exe 'dist\cloudstream-desktop.exe'

    # Optional decoder payload. Drop a libmpv build here to create a bundled
    # (and much larger) installer.
    foreach ($optional in @('libmpv-2.dll', 'mpv-2.dll')) {
        $candidate = Join-Path 'vendor' $optional
        if (Test-Path $candidate) {
            Copy-Item $candidate (Join-Path 'dist' 'libmpv-2.dll')
            Write-Host "Bundling $optional" -ForegroundColor Yellow
            break
        }
    }
    foreach ($doc in @('LICENSE', 'NOTICE.txt')) {
        $candidate = Join-Path $root $doc
        if (Test-Path $candidate) {
            Copy-Item $candidate (Join-Path 'dist' $doc)
        }
    }

    # -- 4. import check ---------------------------------------------------
    $dumpbin = Get-ChildItem -Path "${env:ProgramFiles(x86)}\Microsoft Visual Studio" `
        -Filter dumpbin.exe -Recurse -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -match 'Hostx64\\x64' } |
        Select-Object -First 1
    if ($dumpbin) {
        $imports = & $dumpbin.FullName /dependents $exe | Out-String
        $forbidden = @('python', 'node.exe', 'WebView2Loader', 'vcruntime140d')
        foreach ($name in $forbidden) {
            if ($imports -match [regex]::Escape($name)) {
                throw "Unexpected dependency '$name' in the release binary"
            }
        }
        Write-Host 'Import check passed' -ForegroundColor Green
    }
    else {
        Write-Warning 'dumpbin not found; skipping the import check'
    }

    $size = (Get-Item 'dist\cloudstream-desktop.exe').Length
    Write-Host ("Staged payload: {0:N1} MB" -f ($size / 1MB))

    if ($SkipInstaller) {
        Write-Host 'Skipping installer (--SkipInstaller)' -ForegroundColor Yellow
        return
    }

    # -- 5. installer ------------------------------------------------------
    $iscc = $null
    foreach ($candidate in @(
        "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
        "$env:ProgramFiles\Inno Setup 6\ISCC.exe",
        "${env:LOCALAPPDATA}\Programs\Inno Setup 6\ISCC.exe"
    )) {
        if (Test-Path $candidate) { $iscc = $candidate; break }
    }
    if (-not $iscc) {
        $command = Get-Command ISCC.exe -ErrorAction SilentlyContinue
        if ($command) { $iscc = $command.Source }
    }
    if (-not $iscc) {
        throw 'Inno Setup 6 not found. Install it (winget install JRSoftware.InnoSetup) or pass -SkipInstaller.'
    }

    & $iscc "installer\installer.iss" "/DAppVersion=$Version"
    if ($LASTEXITCODE -ne 0) { throw "ISCC failed with exit code $LASTEXITCODE" }

    $setup = Get-ChildItem 'installer\Output' -Filter '*.exe' |
        Sort-Object LastWriteTime -Descending |
        Select-Object -First 1
    Write-Host ("Installer: {0} ({1:N1} MB)" -f $setup.FullName, ($setup.Length / 1MB)) -ForegroundColor Green

    # -- 6. signing (optional) --------------------------------------------
    if ($Sign) {
        $targets = @($setup.FullName, (Resolve-Path 'dist\cloudstream-desktop.exe').Path)
        foreach ($target in $targets) {
            & signtool sign /fd SHA256 /tr http://timestamp.digicert.com /td SHA256 $target
            if ($LASTEXITCODE -ne 0) { throw "signtool failed for $target" }
        }
        Write-Host 'Signed' -ForegroundColor Green
    }
}
finally {
    Pop-Location
}
