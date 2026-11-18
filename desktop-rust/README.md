# CloudStream Desktop (Rust)

A Windows media centre with a Rust core, a native immediate-mode UI, sandboxed
WebAssembly providers and an Inno Setup installer.

**It ships with no content.** Sources come from providers compiled into the
binary, or from WebAssembly extensions you install — the same extension-first
idea the Android app in this repository uses, with a stricter execution model.

### Built-in providers

`app-providers` holds providers written in Rust and compiled in. `CTGMovies` is a
port of the `CTGMovies` plugin from `Niloy-Sarker/niloy-cloudstream`: films,
series and anime from `ctgmovies.com`, resolved to the direct `.mp4` links libmpv
plays, with the matching `.srt` tracks. **Extensions** lists it with an
Enable/Disable toggle and a **Test** button that runs one live search and reports
what came back (`9 result(s) in 478 ms`, or the error), so a site change shows up
in one click instead of silently returning nothing.

The same check from a terminal:

```bash
cloudstream-desktop --probe builtin-ctgmovies
```

The rest of that repository cannot run here, and nothing is missing from this
build: its plugins are `.cs3` files — a ZIP of Android bytecode that needs Dalvik
and the Android SDK — and three of its five sources (`dflix.discoveryftp.net`,
`cds1.discoveryftp.net`, `play.roarzone.info`) are BDIX-internal names that do not
resolve outside specific Bangladeshi ISPs. The repository is still pre-added on a
fresh profile so the app can say exactly that in **Extensions**, where **Refresh**
retries it and **Remove** deletes the entry. Seeding happens only when no
repository list exists yet; see `BUILTIN_REPOSITORIES` in `app-core`.

> Scope note: this tree is **Windows only**. Android phone, Android TV and
> Chromecast support live in the Kotlin app at the repository root (`app/`,
> `library/`, `shared/`, `desktopApp/`); nothing here replaces them.

---

## Quick start

```bash
# from this directory
cargo run -p app --release
```

The window opens on Home, already loading the built-in provider's shelves.

1. **Search** queries every enabled provider at once; **Extensions** → **Test**
   checks one against its live site
2. Paste a repository URL to add WebAssembly extensions on top
3. Bookmark anything with the bookmark button; resume points are automatic

### Video playback

Video needs `libmpv-2.dll` next to the executable. Without it the app still runs
and the player still works as a timeline (useful for UI work and CI), and
**Settings → Diagnostics** says so plainly instead of failing silently.

```bash
# put the decoder beside the binary, then run
cp /path/to/libmpv-2.dll target/release/
```

The decoder is an optional feature of `app-playback` (`mpv`, on by default) and is
resolved at runtime, never linked. To build a binary that cannot contain the mpv
path at all, disable it at the dependency edge:

```toml
# crates/app/Cargo.toml
app-playback = { workspace = true, default-features = false }
```

---

## Where things live

```
crates/app-core         domain model, MVI reducer, port traits   (serde only)
crates/app-ui           egui shell, screens, overlays, video compositor
crates/app-playback     PlaybackEngine: libmpv (dynamic) + timeline fallback
crates/app-plugin-host  repository indexes, install/verify, WASM sandbox
crates/app-storage      atomic JSON persistence + DPAPI secret vault
crates/app-net          ureq HTTP client behind the HttpClient port
crates/app              binary: wiring, worker threads, --selftest
docs/ARCHITECTURE.md    the design, state machines, risk list
docs/PLUGIN_ABI.md      how to write an extension
docs/DEPENDENCIES.md    dependency audit and size budget
installer/installer.iss Inno Setup 6 script
scripts/build.ps1       build → stage → package
```

## Checks

```bash
cargo test              # 53+ unit tests across the backend crates
cargo clippy -- -D warnings
cargo run -p app -- --selftest   # boots storage, reducer and engines headlessly
```

## Data locations

| What | Where |
|---|---|
| Settings, bookmarks, resume points | `%LOCALAPPDATA%\CloudStreamDesktop\*.json` |
| Installed extensions | `%LOCALAPPDATA%\CloudStreamDesktop\extensions\<hash>\<id>.wasm` |
| Cover cache | `%LOCALAPPDATA%\CloudStreamDesktop\cache\covers\` |
| Provider secrets | same folder, DPAPI-encrypted per Windows user |

Deleting that folder resets the app completely. Nothing is sent anywhere: no
accounts, no telemetry, no analytics, no ads.

## Privacy

The app contacts exactly two kinds of host: the repository URLs you add, and the
stream/image hosts your extensions ask for. There is no update ping, no crash
reporting and no identifier of any kind. `PRIVACY` note also appears in
Settings → Privacy.

## Licence

GPL-3.0-or-later, matching the rest of this repository. libmpv is LGPL-2.1+ when
built without GPL-only components; it is loaded dynamically and shipped
separately, which keeps relinking possible. See `docs/DEPENDENCIES.md`.
