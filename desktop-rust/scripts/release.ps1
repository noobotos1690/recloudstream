<#
.SYNOPSIS
    Publish a GitHub release for CloudStream Desktop: installer + repository index.

.DESCRIPTION
    Replaces the earlier gh-build/ scripts, which could not run:

      * gh-build/local-run/build-dist-windows.bat passed "--%!ASSET!" as a file
        name (a literal leading "--"), so `gh` always failed with "file not found".
      * scripts/release-win.ps1 was named .ps1 but contained bash, and passed
        both --generate-notes and --notes-file, which `gh` rejects.
      * .github/workflows/release-windows.yml sat under desktop-rust/.github,
        where GitHub never looks, and tried to run ISCC.exe (Windows-only) on an
        ubuntu runner.

    Release tagging and CI live in .github/workflows/desktop-rust.yml at the repo
    root; this script is the manual/escape-hatch path and mirrors what CI does.

.PARAMETER Tag
    Release tag, e.g. v1.3.0. Must already exist as an annotated/lightweight tag
    unless you let `gh` create it from the default branch.

.PARAMETER Notes
    Notes file. Defaults to RELEASE_NOTES.md.

.PARAMETER Asset
    Installer to upload. Defaults to the newest installer\Output\CloudStreamDesktop-*.exe.

.PARAMETER Draft
    Create the release as a draft instead of publishing immediately.

.EXAMPLE
    pwsh -File scripts\release.ps1 -Tag v1.3.0
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$Tag,
    [string]$Notes = 'RELEASE_NOTES.md',
    [string]$Asset,
    [switch]$Draft
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Push-Location $root
try {
    # -- installer ----------------------------------------------------------
    if (-not $Asset) {
        $Asset = Get-ChildItem 'installer\Output' -Filter 'CloudStreamDesktop-*.exe' `
            -ErrorAction SilentlyContinue |
            Sort-Object LastWriteTime -Descending |
            Select-Object -First 1 -ExpandProperty FullName
    }
    if (-not $Asset -or -not (Test-Path -LiteralPath $Asset)) {
        throw "No installer found. Run scripts\build.ps1 first, or pass -Asset."
    }

    if (-not (Test-Path -LiteralPath $Notes)) {
        throw "Notes file not found: $Notes"
    }

    # -- assets -------------------------------------------------------------
    # The repository index ships as a release asset so it is served from a real
    # public URL without enabling Pages: releases/download/<tag>/repo.json.
    $files = @((Resolve-Path -LiteralPath $Asset).Path)
    $repoIndex = Join-Path 'releases' $Tag
    $repoIndex = Join-Path $repoIndex 'repo.json'
    if (Test-Path -LiteralPath $repoIndex) {
        $files += (Resolve-Path -LiteralPath $repoIndex).Path
    }
    else {
        Write-Warning "No repository index at $repoIndex; the release will not serve one."
    }

    # WebAssembly extensions. The index lists each binary by a relative name, and
    # `gh release upload` keeps the file's basename, so the artifact is staged
    # under exactly the name the index uses. Otherwise the download 404s while
    # the index still looks correct.
    $wasm = 'plugins\ctgmovies\target\wasm32-unknown-unknown\release\ctgmovies_plugin.wasm'
    if (Test-Path -LiteralPath $wasm) {
        $stage = 'dist\plugins'
        New-Item -ItemType Directory -Force -Path $stage | Out-Null
        $staged = Join-Path $stage 'ctgmovies.wasm'
        Copy-Item -LiteralPath $wasm -Destination $staged -Force
        $files += (Resolve-Path -LiteralPath $staged).Path
    }
    else {
        Write-Warning "No built extension at $wasm; the index will list an asset that is not uploaded."
    }

    Write-Host "Tag:   $Tag"
    Write-Host "Notes: $Notes"
    $files | ForEach-Object { Write-Host "Asset: $_" }

    # -- create or update ---------------------------------------------------
    $null = gh release view $Tag 2>$null
    if ($LASTEXITCODE -eq 0) {
        Write-Host "Release $Tag already exists; updating notes and assets." -ForegroundColor Yellow
        gh release edit $Tag --notes-file $Notes
        if ($LASTEXITCODE -ne 0) { throw "gh release edit failed ($LASTEXITCODE)" }
        gh release upload $Tag @files --clobber
        if ($LASTEXITCODE -ne 0) { throw "gh release upload failed ($LASTEXITCODE)" }
    }
    else {
        $ghArgs = @('release', 'create', $Tag, '--title', $Tag, '--notes-file', $Notes)
        if ($Draft) { $ghArgs += '--draft' }
        $ghArgs += $files
        gh @ghArgs
        if ($LASTEXITCODE -ne 0) { throw "gh release create failed ($LASTEXITCODE)" }
    }

    $url = gh release view $Tag --json url -q .url
    Write-Host "Published: $url" -ForegroundColor Green
}
finally {
    Pop-Location
}
