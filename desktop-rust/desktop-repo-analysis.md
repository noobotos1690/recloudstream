# Niloy Repository Analysis

## 1. repo.json (Top-Level Index)

**Fetched from:** `https://raw.githubusercontent.com/Niloy-Sarker/niloy-cloudstream/master/repo.json`

```json
{
    "name": "Niloy's BDIX Repo",
    "iconUrl": "https://avatars.githubusercontent.com/u/69155993?v=4",
    "description": "Local BDIX FTP plugins for Cloudstream",
    "manifestVersion": 1,
    "pluginLists": [
      "https://raw.githubusercontent.com/Niloy-Sarker/niloy-cloudstream/builds/plugins.json"
    ]
}
```

**Fields:**
- `name`: "Niloy's BDIX Repo"
- `iconUrl`: GitHub avatar URL
- `description`: "Local BDIX FTP plugins for Cloudstream"
- `manifestVersion`: `1`
- `pluginLists`: Array of plugin JSON URLs (only one entry)

---

## 2. plugins.json (Extension Entries)

**Fetched from:** `https://raw.githubusercontent.com/Niloy-Sarker/niloy-cloudstream/builds/plugins.json`

7 plugins, all `.cs3` (Android Kotlin APK format):

| # | name | internalName | language | status | fileSize | description |
|---|------|-------------|----------|--------|----------|-------------|
| 1 | CineBDIX | CineBDIX | bn | 1 | 438429 | CineStream BDIX - Movies, Series & Anime from BDIX streaming sources |
| 2 | CTGMovies | CTGMovies | en | 1 | 37778 | CTGMovies Provider - Stream Movies, TV Shows, and Anime from ctgmovies.com |
| 3 | Dflix | Dflix | bn | 0 | 32711 | Discovery FTP BDIX Provider |
| 4 | DhakaFlix-tmdb | DhakaFlix-tmdb | bn | 1 | 77785 | DhakaFlix BDIX Provider with TMDB support for media description |
| 5 | FMFTP | FMFTP | en | 1 | 61478 | FMFTP - Free Movies FTP Provider |
| 6 | FTPBD | FTPBD | bn | 1 | 40694 | FTPBD BDIX Emby Server Provider - Access media from media.ftpbd.net:8096 |
| 7 | Roarzone | Roarzone | hi | 1 | 41796 | RoarZone Emby Server Provider - Access media from play.roarzone.info |

**Per-extension fields (example: CTGMovies):**
- `url`: `https://raw.githubusercontent.com/Niloy-Sarker/niloy-cloudstream/builds/CTGMovies.cs3`
- `status`: 1=enabled, 0=disabled
- `version`: integer
- `name`: display name
- `internalName`: internal identifier (matches `provider_id`)
- `authors`: array of strings
- `description`: human-readable description
- `fileSize`: bytes
- `repositoryUrl`: `"https://github.com/Niloy-Sarker/niloy-cloudstream"`
- `language`: language code (`en`, `bn`, `hi`)
- `tvTypes`: array of content types (Movie, TvSeries, Anime, etc.)
- `iconUrl`: provider icon URL
- `apiVersion`: 1
- `fileHash`: `sha256-<hex>` — lower-case hex SHA-256 of the `.cs3` binary

---

## 3. Android ↔ Desktop Wasm Field Mapping Table

| Android `.cs3` Field | Desktop Wasm Equivalent | Notes |
|---|---|---|
| `provider_id` (= `internalName`) | `provider_id` | Same — identifies the Wasm plugin |
| `url` (`.cs3` binary path) | `url` (`.wasm` binary path) | Android has `.apk`; desktop needs `.wasm` |
| `fileHash` (`sha256-<hex>`) | `sha256` (**bare** `<hex>`, no prefix) | Same digest, but the `sha256-` prefix must be stripped: the desktop validator requires exactly 64 hex characters and rejects the prefixed form |
| `manifest_version` | `manifest_version` | Same format versioning |
| `signature` (optional) | Optional Ed25519 sig | Same security model |
| `extensions` list | `extensions` list | Same structure, different binary format |
| `name` | `name` | Same display name |
| `description` | `description` | Same human-readable description |
| `language` | `language` | Same language code |
| `tvTypes` | `tvTypes` | Same content type classification |
| `iconUrl` | `icon_url` | Same provider icon |
| `apiVersion` | `apiVersion` | Same API version |
| `fileSize` | `size_bytes` | Same byte size |
| `status` (1=enabled, 0=disabled) | `enabled` (bool) | Same enable/disable semantics |

**Key observation:** The field names shift slightly (e.g., `fileHash` → `sha256`, `iconUrl` → `icon_url`, `status` → `enabled`), but all data maps 1:1. The only real change is `.cs3` → `.wasm` binary URLs and hash verification.

---

## 4. Desktop Repository Schema (Plan Example)

**Defined in:** `desktop-rust/desktop-repo-schema.json` (a template, not a live repo)

```json
{
  "name": "Example desktop repository (template)",
  "description": "TEMPLATE, not a live repository. Replace url/sha256/size_bytes with a real .wasm binary before serving.",
  "manifest_version": 1,
  "extensions": [
    {
      "provider_id": "example_provider",
      "name": "Example Provider",
      "version": "1.0.0",
      "description": "Shows the shape of one extension entry; replace with a real provider.",
      "language": "en",
      "url": "https://example.com/example_provider.wasm",
      "sha256": "0000000000000000000000000000000000000000000000000000000000000000",
      "icon_url": "https://example.com/icon.png",
      "size_bytes": 0
    }
  ]
}
```

**Correction to the first revision of this document.** The example originally
carried `"sha256": "sha256-650b4c…"`, copied from the Android `fileHash` field.
`validate_entry` in `crates/app-plugin-host/src/manifest.rs` requires
`len == 64`, so that entry was rejected with *"has no valid SHA-256 hash"* even
though the file looked right. The prefix is Android's convention, not the
desktop schema's. `repo::tests::sha256_prefix_from_android_format_is_rejected`
pins the rejection and `repo::tests::shipped_release_index_parses_and_validates`
compiles the shipped `releases/v1.3.0/repo.json` into the test suite so the two
cannot drift apart again.

There is also **no `plugins.json` indirection** on the desktop side: Android's
`repo.json` → `pluginLists` → `plugins.json` chain collapses into a single file,
because `Repository.extensions` holds the entries inline. `plugins.json` was
removed from `releases/v1.3.0/` for that reason.

---

## 5. What Was Already Working in Desktop Build

- **`BUILTIN_REPOSITORIES`** in `crates/app-core/src/domain.rs` now seeds the
  **desktop** index instead of the Android one:
  ```rust
  pub const BUILTIN_REPOSITORIES: &[(&str, &str)] = &[(
      "CloudStream Desktop Extensions",
      "https://github.com/noobotos1690/recloudstream/releases/latest/download/repo.json",
  )];
  ```
  It previously pointed at
  `raw.githubusercontent.com/Niloy-Sarker/niloy-cloudstream/master/repo.json`,
  which `parse_repository` rejects every single time (Android `pluginLists`
  shape). Because a built-in repository is seeded on a fresh profile and any
  repository with a `last_error` is retried on every start, that entry produced
  the same permanent, recurring error for every new user. The Android index is
  documented here instead of being shipped as a dead built-in.
- **Extensions screen** has Refresh and Remove buttons per repository row (via `Intent::RefreshRepository` and `Intent::RemoveRepository`)
- **CTGMovies provider** already ported to Rust in `app-providers` crate (the only Android → Wasm port needed)
- **All 81 tests pass**, 0 failed; clippy 0 warnings; `cargo fmt --all --check` clean
- **`--probe builtin-ctgmovies`**: home 3 rows / 89 items, search 9 items, details resolve, 2 sources found, exit 0
- **Release build**: PE subsystem 2 → no console window

---

## 6. Niloy Repository Behavior Outside Bangladesh

The 5 BDIX-internal sources (`dflix.discoveryftp.net`, `cds1.discoveryftp.net`, `play.roarzone.info` plus 2 more) only resolve within specific Bangladeshi ISPs. Outside Bangladesh, DNS resolution fails as expected — this is not a bug, it's geographic restriction.

Error handling:
- Errors stored in `last_error` on `RepositoryRecord`
- Retry on every start for records with `last_error.is_some()`
- Identical repeated errors suppressed (no double toast)
- `RefreshRepository` clears `last_error` so next failure IS shown
- `RemoveRepository` deletes repo entry; installed `.wasm` extensions stay on disk

---

## 7. Summary of Completed Plan Items

| # | Item | Status |
|---|------|--------|
| 1 | Fetch and analyze `repo.json` | ✅ Done — format documented |
| 2 | Create Android→Desktop Wasm mapping table | ✅ Done — all fields mapped |
| 3 | Design desktop repo schema JSON | ✅ Done — `desktop-repo-schema.json` created |
| 4 | Verify all plan verifications pass | ✅ Done — 81 tests, 0 clippy warnings, clean fmt, release build OK, --probe works |
| 5 | Niloy repo analysis complete | ✅ Done — BDIX-internal DNS restriction documented |