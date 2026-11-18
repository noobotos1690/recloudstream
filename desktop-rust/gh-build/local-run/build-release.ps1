# PowerShell wrapper around build-dist-windows.bat.
# We cannot execute the Inno Setup (ISCC.exe) build step from here, so
# build-dist-windows.bat is a stub: it checks for an existing
# CloudStreamDesktop-*.exe under installer\Output\ and, if found, calls
# `gh release create v1.3.0` with that asset and RELEASE_NOTES.md.
#
# To perform a real build you would run ISCC.exe against
# installer\installer.iss first, then run this script.
# Requires `gh` on PATH and a token with repo write permission.
#
# The GH_TOKEN is passed through so the bat can authenticate with the
# GitHub CLI for the release action.

param(
    [ValidateSet("build","release","all")]
    [string]$Mode = "release"
)

$ErrorActionPreference = "Stop"
$root = Split-Path $MyInvocation.MyCommand.Path
$script = Join-Path $root "build-dist-windows.bat"

if (-not (Get-Command gh -ErrorAction SilentlyContinue)) {
    throw "gh CLI not found on PATH; cannot release to GitHub."
}

if ($env:GH_TOKEN) {
    $env:GH_TOKEN = $env:GH_TOKEN
}

if ($Mode -in @("build","all")) {
    Write-Host "[stub] Inno Setup build step would run here (ISCC.exe installer\installer.iss). Not executed."
    Write-Host "[stub] For a real release you must build the installer first, then re-run this script."
}

if ($Mode -in @("release","all")) {
    Write-Host "Running release script: $script"
    & cmd /c "`"$script`""
}

Write-Host "Done. Release notes are in RELEASE_NOTES.md; the release YAML workflow is in .github/workflows/"
