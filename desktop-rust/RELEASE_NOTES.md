# v1.3.0 — CloudStream Desktop (Windows)

## What is ready now (released)

- A standalone Windows desktop build of CloudStream with a **built-in CTGMovies** provider.
- Extensions screen with per-row **Refresh** and **Remove** buttons for each repository.
- Repository retry logic: a repository that never loaded successfully is retried on every start; repeated identical errors are suppressed so the user is not greeted by the same toast over and over.
- Release build uses `windows_subsystem = "windows"` so the app starts without a console window.
- All tests pass (`cargo test --all` — **80 passed, 0 failed**), clippy is clean (`0 warnings`), and `cargo fmt --all --check` is clean.
- A Windows installer is attached to this release: `CloudStreamDesktop-0.1.0-setup.exe` (~5.9 MB, LZMA2).
- The repository index `repo.json` is attached to this release and is served from a real public URL:
  `https://github.com/noobotos1690/recloudstream/releases/download/v1.3.0/repo.json`

## How to test

1. Build: `cargo build --release --locked --target x86_64-pc-windows-msvc` from `desktop-rust/`
2. Smoke test: `CS_CHECK=1 ./target/x86_64-pc-windows-msvc/release/cloudstream-desktop.exe --probe builtin-ctgmovies`
3. Or run the whole chain (build, stage, verify imports, compile installer): `pwsh -File scripts/build.ps1`
4. The installer is written to `desktop-rust/installer/Output/CloudStreamDesktop-<version>-setup.exe`

## What is NOT ready — the repository is valid but empty

The Niloy repository integration is **documented, not runnable**, and the index
shipped with this release is **valid but contains no installable extensions**.

Specifically:

- We fetched and analyzed the original `repo.json` + `plugins.json` from `Niloy-Sarker/niloy-cloudstream`.
- We produced a **field mapping table** showing how Android `.cs3` (APK) extension fields map to desktop `.wasm` extension fields.
- The Android index cannot be used as-is: `parse_repository` detects the `pluginLists`/`manifestVersion` shape and refuses it with *"this is a CloudStream Android repository: its plugins are Kotlin APKs, while this desktop build runs WebAssembly extensions"*.
- There are **no desktop `.wasm` binaries in this repository at all**. The only provider that works is **CTGMovies**, and it is compiled into the binary as a **built-in Rust provider**, not installed from the index.
- The other six original plugins (CineBDIX, Dflix, DhakaFlix-tmdb, FMFTP, FTPBD, Roarzone) are still **Android-only `.cs3`** plugins and have no desktop `.wasm` equivalent yet.

So `releases/v1.3.0/repo.json` deliberately declares `"extensions": []`. It is a
real, schema-valid index that the app can fetch and parse — it proves the
distribution path works end to end — but it installs nothing, because there is
nothing truthful to install yet.

> **Correction to an earlier draft of these notes.** The previous `repo.json`
> listed a CTGMovies entry whose `sha256` began with `sha256-`, copied from
> Android's `fileHash`. The desktop validator requires a bare 64-character hex
> digest, so that file was rejected outright rather than installing. It has been
> replaced, and two regression tests now pin the behaviour
> (`shipped_release_index_parses_and_validates`,
> `sha256_prefix_from_android_format_is_rejected`).

## How the public endpoint works

The index is served straight from the release assets — no Pages, no object
storage, no server to keep alive:

```
https://github.com/noobotos1690/recloudstream/releases/download/v1.3.0/repo.json
```

When real `.wasm` binaries exist, attach them to the same release (or a later
one) and point each entry's `url` at its own `releases/download/...` address,
with the binary's bare SHA-256 in `sha256`. Nothing about the endpoint has to
change.

For local testing, `docker/docker-compose.yml` serves the same directory over
`http://127.0.0.1:8080/repo/repo.json`.

## Repo notes

- `installer/Output/`, `target/` and `dist/` are ignored. The installer is built by
  CI (or `scripts/build.ps1`) and attached to the release, never committed.
- `plugins.json` has been removed: the desktop schema has no plugin-list
  indirection. `Repository.extensions` holds the entries inline, so Android's
  `repo.json` → `pluginLists` → `plugins.json` chain collapses to one file.
- Release automation lives in `.github/workflows/desktop-rust.yml` at the repo
  root (the nested `desktop-rust/.github/workflows/` copy never ran — GitHub only
  reads workflows from the root). The manual path is `scripts/release.ps1`.
- This repo is the desktop project under `desktop-rust/`. The Android project
  lives in the parent repo and is not touched here.

## Known limits

- No installable desktop extension exists yet; the only provider is built in.
- The desktop build cannot install Android `.cs3` plugins and should not pretend to.
- BDIX-internal sources only resolve inside specific Bangladeshi ISPs; failures
  elsewhere are geographic restriction, not a bug.

— CloudStream Desktop v1.3.0
