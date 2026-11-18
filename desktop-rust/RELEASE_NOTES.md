# v1.3.0 — CloudStream Desktop (Windows)

## What this release is
A first standalone Windows desktop build of CloudStream with a built-in **CTGMovies** provider, a working Extensions screen (refresh + remove per repository row), and a packaged Windows installer. All tests pass and the build is checked in.

## What works
- `--probe builtin-ctgmovies` loads the built-in CTGMovies provider and returns home/search/details/resolve results
- Extensions screen shows repository rows with per-row Refresh and Remove buttons
- Repository retry logic: a repo that never loaded successfully is retried on every start, with repeated identical errors suppressed
- Release build has `windows_subsystem = "windows"` so it opens without a console window

## What is not yet ready — only a desktop schema artifact right now
The **Niloy repository integration is documented, not yet runnable.**

What exists today is an **analysis and schema artifact**, not a live desktop repository:
- We fetched and analyzed the original `repo.json` + `plugins.json` from `Niloy-Sarker/niloy-cloudstream`
- We built a **mapping table** showing how Android `.cs3` (APK) extension fields map to desktop `.wasm` extension fields
- We wrote a **sample desktop `repo.json`** (`desktop-repo-schema.json`) showing the shape, but its Wasm `.wasm` URLs are **placeholders**, not real hosted binaries with verified SHA-256 hashes

In particular, the original Niloy repo distributes **Android `.cs3` (APK) plugins**, while this desktop build runs **WebAssembly (.wasm) extensions**. Only **one of the original plugins** — CTGMovies — has a real desktop counterpart today (the built-in Rust provider), and that one is already compiled into the binary. The other six original plugins do **not** exist as desktop `.wasm` binaries yet, so the desktop repo schema we wrote cannot be used as a real, installable repository today — the `.wasm` URLs are placeholders and the hashes would not match any real binary.

This is a schema/analysis artifact: it shows the target shape and the exact field mapping, but there is no public endpoint serving real desktop repo.json/plugins.json yet. A usable desktop repo still requires either real compiled `.wasm` binaries with matching SHA-256 hashes, or a build pipeline that produces them.

## Roadmap notes
- Real desktop repo.json/plugins.json: needs real `.wasm` binaries (with matching SHA-256) hosted at public URLs, or a build pipeline that produces them. The CTGMovies provider is the first candidate since it already exists as a built-in Rust provider.
- Other Niloy plugins (CineBDIX, Dflix, DhakaFlix-tmdb, FMFTP, FTPBD, Roarzone): not yet ported to `.wasm`; they remain Android-only `.cs3` plugins. These do not currently have desktop equivalents.

## How to test
1. Build: `cargo build --release --locked --target x86_64-pc-windows-msvc` from `desktop-rust/`
2. Smoke test: `CS_CHECK=1 ./target/x86_64-pc-windows-msvc/release/cloudstream-desktop.exe --probe builtin-ctgmovies`
3. The installer lives at `desktop-rust/installer/Output/CloudStreamDesktop-0.1.0-setup.exe`

## Repo notes
- The `installer/Output/` directory is intentionally **not committed** to Git by default (it is in `.gitignore`). The installer binary was added manually for this release only.
- This repo is the desktop project under `desktop-rust/`. The Android project lives in the parent repo and is not touched here.

## Known limits
- This is a desktop schema/analysis artifact for the Niloy repo integration, not a complete, usable desktop repository today.
- Only CTGMovies has a real desktop provider (and only as a built-in, not as an installable `.wasm` from the schema repo yet).

— CloudStream Desktop v1.3.0
