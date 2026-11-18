# v1.3.0 — CloudStream Desktop (Windows)

## What is ready now (released)

- A standalone Windows desktop build of CloudStream with a **built-in CTGMovies** provider.
- **A working, installable WebAssembly extension**: `ctgmovies.wasm` is attached to this
  release, listed in the repository index, and installs through the normal Extensions
  screen. It is a real wasm32 module — not a repackaged Android plugin.
- Extensions screen with per-row **Refresh** and **Remove** buttons for each repository.
- Repository retry logic: a repository that never loaded successfully is retried on every start; repeated identical errors are suppressed so the user is not greeted by the same toast over and over.
- Release build uses `windows_subsystem = "windows"` so the app starts without a console window.
- All tests pass (`cargo test --all` — **84 passed, 0 failed**), clippy is clean (`0 warnings`), and `cargo fmt --all --check` is clean.
- A Windows installer is attached: `CloudStreamDesktop-0.1.0-setup.exe` (~5.9 MB, LZMA2).

## Add the repository

Paste this into the Extensions screen and press **Add repository**:

```
https://github.com/noobotos1690/recloudstream/releases/latest/download/repo.json
```

The field needs a **full http/https URL**; typing a name such as `cs-niloy` is rejected
with *"Repository URLs must start with http:// or https://"*. A profile that has never
stored a repository gets this entry pre-added automatically.

The index lists one extension:

| provider_id | name | binary |
|---|---|---|
| `ctgmovies` | CTGMovies | `ctgmovies.wasm` (172,438 B) |

## What is NOT ready — the other six Niloy plugins

The Niloy repository's own index **cannot** be used here and never will be forwarded
to: it is an Android CloudStream repository, its entries are Kotlin `.cs3` packages, and
this runtime executes wasm32 modules. `parse_repository` says so explicitly instead of
failing with a schema error.

Of the seven plugins in `Niloy-Sarker/niloy-cloudstream`:

- **CTGMovies** is the only one with a desktop implementation (two of them: the built-in
  and the installable extension above).
- **CineBDIX, Dflix, DhakaFlix-tmdb, FMFTP, FTPBD, Roarzone** are still Android-only.
  Each needs its own reimplementation in Rust + `host_http_request`; none can be
  repackaged from the `.cs3`. There is no shortcut and no automated converter.
- The BDIX-internal sources only resolve inside specific Bangladeshi ISPs, so even a port
  of those would fail to load outside Bangladesh — that is geographic restriction, not a bug.

## How to test

1. Build: `cargo build --release --locked --target x86_64-pc-windows-msvc` from `desktop-rust/`
2. Smoke test: `CS_CHECK=1 ./target/x86_64-pc-windows-msvc/release/cloudstream-desktop.exe --probe builtin-ctgmovies`
3. Whole chain (build, stage, verify imports, compile installer): `pwsh -File scripts/build.ps1`
4. Build the extension: `pwsh` from `plugins/` — see `plugins/README.md`
5. Publish a release with installer + index + binary: `pwsh -File scripts/release.ps1 -Tag v1.3.0`

Verified end to end with the application's own code: fetch the index from the public URL,
resolve the entry, download the binary, match the pinned SHA-256, load it in the sandbox,
and run `search` (9 items). The probe independently returns home 3 rows / 89 items,
search 9 items, details resolved and 2 stream sources, exit 0.

## Corrections to earlier revisions of this release

These were real defects, fixed in this build:

- The shipped `repo.json` used Android's `"sha256-<hex>"` form. The validator requires a
  bare 64-character digest, so the index was rejected outright rather than installing.
- `validate_entry` rejected relative entry URLs even though `HttpRepository::resolve`
  implements them and `docs/PLUGIN_ABI.md` documents them. Relative URLs now validate;
  the shipped index still uses absolute URLs so older builds keep working.
- `plugins.json` was removed: the desktop schema has no `pluginLists` indirection.
- `docker/docker-compose.yml` mounted `./releases/v1.3.0`, which resolves to
  `docker/releases/v1.3.0` and never existed, so the volume came up empty.
- Three release scripts could not run: a `.ps1` containing bash that also passed
  `--generate-notes` with `--notes-file`, a `.bat` passing `--%!ASSET!` as a filename,
  and a nested workflow GitHub never reads that called `ISCC.exe` on ubuntu. Replaced by
  `scripts/release.ps1` and the root `desktop-rust.yml` workflow.
- `BUILTIN_REPOSITORIES` seeded Niloy's Android index, which can never load, so every
  fresh profile carried a permanent, retried error. It now seeds the desktop index.
- The installer tests shared one temp directory keyed only by process id, so the
  parallel tests deleted each other's files and the suite failed intermittently. Each
  test now gets its own root.

## Notes

- `installer/Output/`, `target/` and `dist/` are ignored. Releases are built by CI or
  `scripts/build.ps1` and attached as assets, never committed.
- The extension reuses the built-in `CtgMovies` provider rather than copying it, so the
  two cannot drift. It reports `ctgmovies` rather than `builtin-ctgmovies` because the
  registry replaces providers on id collision.
- This repo is the desktop project under `desktop-rust/`. The Android project lives in
  the parent repo and is not touched here.

— CloudStream Desktop v1.3.0
