# Porting the Android app to desktop

Measured on 2026-11-18 in this repository. Counts are `grep`/`wc` results; sizes are
build outputs. Nothing here is estimated unless it says so.

## What the original is, in numbers

| Module | Files | Lines | Notes |
|---|---|---|---|
| `app` | 275 | ~70,000 | Android-only: activities, media3, WorkManager, WebView, DEX loading |
| `app/ui` | 159 | 49,510 | 114 of these files import `android.*` |
| `app/ui/player` | 38 | 17,280 | media3 / ExoPlayer; 30 files Android-bound |
| `shared` | 56 | 8,256 | Compose Multiplatform UI kit; **0** `android.*` imports; `jvm()` target |
| `shared` platform seams | — | — | exactly **2** `expect` declarations (`ClipboardHelper`, `DevicePreferenceStore`), both with `.jvm.kt` actuals |
| `library` | — | — | plugin API; targets `android`, `jvm`, **`webMain` (wasm)**; 0 `android.*` in `commonMain` |

Android-only API surface in `app`: `DownloadManager` in 20 files, `WebView` in 15,
`WorkManager` in 7, `NotificationCompat` in 7, `PathClassLoader` in `PluginManager.kt:611`.

## Verdict

The APK cannot run on Windows: it is DEX bytecode against the Android framework. The
*codebase* can run, because the portable half is already multiplatform and the module
that matters most for "the same app" (`shared`) is Android-free.

## Proof: the plugin gate

A real plugin, `DailymotionProvider.cs3` (11,472 bytes), was taken from the official
repository and loaded on the desktop JVM:

1. `manifest.json` read straight out of the zip — `{pluginClassName: recloudstream.DailymotionPlugin, version: 4}`.
2. `classes.dex` (26,036 bytes) translated to JVM classes with dex2jar 2.4.
3. Loaded through `URLClassLoader`, `BasePlugin.load()` called.
4. Result: provider `Dailymotion` registered, `getMainPage()` returned **1 row, 26 items** from the live API. `InternetArchiveProvider` likewise: **1 row, 26 items**.
5. End-to-end through the app's own host: repository fetch (**5 plugins**) → install → SHA-256 → translate → load → **26 items** → disable → **0 providers**.

`search()` on both providers throws `NotImplementedError` — the base implementation, not
a port bug; the desktop host catches it per provider.

So: DEX plugins are answerable on desktop. What is *not* proved is that every plugin
works: any plugin calling `android.*` (WebView challenges, Android context) will fail at
that call. That is the remaining risk, and it is per plugin, not per host.

## Decisions taken

**Plugin loading: dex2jar + `URLClassLoader`** (implemented). Bundle dex2jar (Apache-2.0)
and keep a cache keyed by the plugin's SHA-256, so a plugin is translated once.
Alternatives rejected: a dex2jar-free hand-written DEX→class translator (months), or
requiring plugin authors to publish JVM jars (breaks the ecosystem).

**Player: not decided, not built.** The two real candidates:
- **libmpv via JNI** — best format coverage and the same engine mpv users expect; needs a
  native binary per platform and a thin JNI/SWIG layer.
- **VLCJ** — pure-JVM binding, easier packaging, but pulls a ~120 MB runtime and its
  rendering path on Windows is less predictable.

Both replace 17,280 lines of media3 UI, and neither is a drop-in: the player UI has to be
rebuilt either way. This is the single largest remaining task.

**Packaging.** Compose Desktop's `createDistributable` produces a JVM image; an MSI needs
WiX. The current `desktop-rust` build is 11.4 MiB for comparison — a JVM image will be
an order of magnitude larger. That is the cost of running the same code.

## What exists in `desktopApp` now

- Host: `host/{Models,Http,DexTranslator,PluginStore,PluginHost,HeadlessCheck}.kt`.
- UI: `DesktopApp.kt` — Home, Search, Result, Extensions (repositories, install, enable,
  disable, remove, local folder import), styled by `shared`'s `CloudStreamTheme`.
- `--check` / `CS_CHECK=1` runs the whole pipeline headless.
- Tests: `PluginHostTest` (URL normalisation, wire formats, state round-trip, hashing).

## Known gaps

- Plugin **updates** are not diffed against repository versions yet.
- Repositories are fetched by URL only; Android's shortcodes (`py.md`, `cutt.ly`) are not implemented.
- Result pages show metadata but not episodes/sources.
- Subtitle tracks, sync providers, downloads, cast: not started.
- `EpisodeDateTest` fails in `:library:jvmTest` (2 of 20 cases) because it assumes a fixed
  timezone; it is unrelated to this port and was failing before it.
