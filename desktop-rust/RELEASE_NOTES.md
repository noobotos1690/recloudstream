# v1.3.0 — CloudStream Desktop (Windows)

## What is ready now (released)
- A standalone Windows desktop build of CloudStream with a **built-in CTGMovies** provider.
- Extensions screen with per-row **Refresh** and **Remove** buttons for each repository.
- Repository retry logic: a repository that never loaded successfully is retried on every start; repeated identical errors are suppressed so the user is not greeted by the same toast over and over.
- Release build uses `windows_subsystem = "windows"` so the app starts without a console window.
- All tests pass (`cargo test --all` — 84 passed, 0 failed), clippy is clean (`0 warnings`), and `cargo fmt --all --check` is clean.
- A Windows installer is attached to this release: `CloudStreamDesktop-0.1.0-setup.exe` (6,162,788 B).

## How to test
1. Build: `cargo build --release --locked --target x86_64-pc-windows-msvc` from `desktop-rust/`
2. Smoke test: `CS_CHECK=1 ./target/x86_64-pc-windows-msvc/release/cloudstream-desktop.exe --probe builtin-ctgmovies`
3. The installer lives at `desktop-rust/installer/Output/CloudStreamDesktop-0.1.0-setup.exe`

## What is NOT ready — this is a desktop schema artifact only
The Niloy repository integration is **documented, not yet runnable**, and the files added here are **schema artifacts**, not a live installable desktop repository.

Specifically:
- We fetched and analyzed the original `repo.json` + `plugins.json` from `Niloy-Sarker/niloy-cloudstream`.
- We produced a **field mapping table** showing how Android `.cs3` (APK) extension fields map to desktop `.wasm` extension fields.
- We wrote a **sample desktop `repo.json`** (`desktop-repo-schema.json`) showing the target shape, but its `.wasm` URLs are **placeholders** and there are no real desktop `.wasm` binaries with verified SHA-256 hashes.
- The only original plugin that has a real desktop counterpart is **CTGMovies**, and that one is already compiled into the binary as a **built-in Rust provider** — it is **not** served as an installable `.wasm` from the schema repo yet.
- The other six original plugins (CineBDIX, Dflix, DhakaFlix-tmdb, FMFTP, FTPBD, Roarzone) are still **Android-only `.cs3`** plugins and have no desktop `.wasm` equivalent yet.

So today, the desktop repo schema cannot be used as a real installable repository: the `.wasm` URLs are placeholders and the hashes would not match any real binary. A usable desktop repo still requires either real `.wasm` binaries with matching SHA-256 hashes hosted at public URLs, or a build pipeline that produces them.

## Public endpoint plan (design, not yet live)
The intended way to serve real desktop `repo.json` / `plugins.json` is documented in the source tree:
- `docker/docker-compose.yml` — an Nginx service that serves `releases/v1.3.0/repo.json` and `releases/v1.3.0/plugins.json` on `http://127.0.0.1:8080/repo/`.
- `releases/v1.3.0/repo.json` and `releases/v1.3.0/plugins.json` — the schema files (currently with placeholder `.wasm` URLs).

This is a **design**, not a live public endpoint yet. Before this can be used as a real repository, someone needs to either host the `.wasm` binaries at real URLs with correct SHA-256 hashes, or run the pipeline that produces them.

## Repo notes
- The `installer/Output/` directory is intentionally **not committed** to Git by default (it is in `.gitignore`). The installer binary was added manually for this release only and is tracked in this commit.
- This repo is the desktop project under `desktop-rust/`. The Android project lives in the parent repo and is not touched here.

## Known limits
- This is a desktop schema/analysis artifact for the Niloy repo integration, not a complete, usable desktop repository today.
- Only CTGMovies has a real desktop provider (and only as a built-in, not as an installable `.wasm` from the schema repo yet).

— CloudStream Desktop v1.3.0
