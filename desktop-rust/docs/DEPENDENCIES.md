# Dependency audit

The installer budget is the binding constraint, so every dependency below was
chosen against its alternatives rather than by default. Sizes are **estimates**
for a release build with the profile in the workspace `Cargo.toml`; run the
commands at the end to produce real numbers on your machine.

## Direct dependencies

| Crate | Version | Why | Est. contribution |
|---|---|---|---|
| `serde` + `serde_json` | 1.x | State, settings, plugin ABI, repository index. Non-negotiable. | ~250 KB |
| `eframe` / `egui` (+`glow`) | 0.31 | Native immediate-mode UI, OpenGL backend. | ~4–5 MB |
| `image` | 0.25 (jpeg/png/webp) | Cover art decode. WebP only because many image CDNs serve it. | ~800 KB |
| `ureq` (rustls) | 3.5 | Blocking HTTP. | ~600 KB + TLS |
| `extism` (wasmtime, cranelift) | 1.41 | The plugin sandbox. Largest single dependency. | ~6–7 MB |
| `sha2` | 0.10 | Manifest hash verification. | ~40 KB |
| `ed25519-dalek` | 2.1 | Optional-but-verified repository signatures. | ~120 KB |
| `libloading` | 0.8 | Runtime load of `libmpv-2.dll` — keeps it optional. | ~15 KB |

No `windows` / `windows-sys` crate: the two DPAPI calls are declared directly
against `crypt32.dll`, which costs nothing and links against a DLL that ships with
Windows.

## Deliberate exclusions

| Not used | Instead | Reason |
|---|---|---|
| `tokio` / `async-std` | `ureq` on dedicated threads | An async runtime is ~1.5 MB and a second scheduler for a handful of short requests. |
| `reqwest` | `ureq` | `reqwest` pulls hyper + tokio; ~4 MB for the same result. |
| `rusqlite` / SQLite | JSON files | Largest dataset is a few hundred bookmarks; SQLite is ~1.5 MB and a DLL for queries we never run. |
| WebView2 / Tauri | egui + glow | WebView2 is a ~100 MB shared runtime on older systems and adds first-frame latency. |
| `html5ever` / `scraper` in the host | guest-side `tl` parser | Parsing belongs in the plugin; keeps ~2 MB out of the host. |
| `anyhow` / `thiserror` | hand-written `Display`/`Error` | Two enums, ~40 lines, zero macro dependencies. |
| UPX | nothing | UPX-packed binaries are routinely flagged by Defender. Signed, LTO-stripped builds instead. |
| `rayon` | `std::thread` per search fan-out | Provider fan-out is 1–10 threads; a pool is unnecessary. |

## External binaries

| Component | Bundled? | Est. size |
|---|---|---|
| `libmpv-2.dll` + FFmpeg | **Optional.** Dropped into `vendor/` by the packager, loaded at runtime. | 25–35 MB |
| MSVC runtime | No — statically linked by Rust's MSVC target. | 0 |
| SQLite DLL | Not used. | 0 |
| WebView2 | Not used. | 0 |

## Size budget vs. reality

| Configuration | Size |
|---|---|
| Bring-your-own libmpv (default build) | **11.39 MiB binary, 5.88 MiB installer — measured** |
| Bundled libmpv | +25–35 MB (estimate, log-free FFmpeg build) |
| With `codegen-units=16` instead of 1 (faster build) | +1–2 MB (estimate) |

Both targets are met in the default configuration. The installer is 5.88 MiB
(6,162,592 bytes) for an 11.39 MiB payload, so LZMA2 solid recovers 48 % — and the
whole download is smaller than the 15 MB stretch target. Bundling libmpv is what
breaks both targets, which is why the packager treats it as opt-in.

## Measuring it yourself

```bash
# 1. real binary size
cargo build --release --target x86_64-pc-windows-msvc
ls -l target/x86_64-pc-windows-msvc/release/cloudstream-desktop.exe

# 2. per-crate contribution (needs `cargo install cargo-bloat`)
cargo bloat --release --target x86_64-pc-windows-msvc -n 30

# 3. duplicate dependency versions
cargo tree --duplicates

# 4. actual installer size
powershell -ExecutionPolicy Bypass -File scripts/build.ps1
ls -l installer/Output
```

`scripts/build.ps1` also runs `dumpbin /dependents` on the staged exe and fails
the build if an unexpected import (WebView2, a debug CRT, a Python DLL) appears.

## Licensing notes

* Application: GPL-3.0-or-later, matching the rest of this repository.
* `libmpv` is LGPL-2.1+ **when built without GPL-only components**. It is loaded
  with `libloading` and distributed separately from the application, which
  satisfies LGPL's relinking requirement. If you bundle a GPL-enabled libmpv
  build, the combined distribution must be GPL-compatible — which it is here,
  since the app itself is GPL-3.0-or-later.
* Extension binaries are **not** linked into the host. They are data loaded by a
  runtime, so a provider's licence does not affect the application's licence.
