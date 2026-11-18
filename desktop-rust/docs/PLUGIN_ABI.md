# Plugin ABI v1

An extension is a WebAssembly module (`wasm32-unknown-unknown`, no WASI) that
exports a handful of functions. The host instantiates one module per provider and
talks to it with JSON documents.

Everything here is deliberately boring: JSON in, JSON out, one error envelope.
That keeps the ABI debuggable with `cat` and lets providers be written in Rust,
Go, AssemblyScript or Zig.

---

## 1. Calling convention

The host uses [Extism]'s calling convention: each export takes one `i64` (a packed
pointer/length to the input block in guest memory) and returns one `i64` (the same
packing for the output block). You should not implement that by hand — use a PDK.

Rust providers use `extism-pdk`:

```rust
use extism_pdk::*;

#[plugin_fn]
pub fn search(input: String) -> FnResult<String> { … }
```

The host resolves the module, calls the export by name, reads the returned bytes,
and parses them as an [`Envelope`].

### Exports the host looks for

| Export | Input | Output (inside `Envelope`) |
|---|---|---|
| `provider_manifest` | `""` (empty) | `ProviderManifest` — called once at load; a garbage manifest refuses the extension |
| `home` | `{}` | `Vec<HomeRow>` |
| `search` | `{"query": "…"}` | `Vec<MediaItem>` |
| `details` | `{"id": "…"}` | `MediaDetails` |
| `resolve` | `{"id": "…", "episode_id": "…"?, "preferred_quality": "fhd1080"}` | `Vec<StreamSource>` |

An export the manifest does not claim (e.g. `home` when `capabilities.home` is
false) is never called; the host returns `unsupported` instead.

---

## 2. The envelope

Every reply is:

```json
{ "ok": true,  "data": <payload> }
```

or

```json
{ "ok": false, "kind": "not_found", "error": "no match for 'dune'" }
```

`kind` is one of `not_found`, `network`, `unsupported`, `parse`, `sandbox`,
`failed`. The host maps it onto its own error type, which decides how the UI
reacts: `network` is retryable, `unsupported` hides a button, `not_found` shows an
empty state.

Returning `{ "ok": false }` is always correct and always better than trapping: a
trap becomes an opaque `sandbox` error and the user loses the reason.

---

## 3. Payload shapes

Field names match `app-core::domain` exactly — the same structs serialise on both
sides, so there is no second copy of the schema to drift.

```jsonc
// MediaItem
{
  "provider_id": "demo",       // must equal the id in provider_manifest
  "id": "tt1160419",           // provider-scoped; opaque to the host
  "title": "Dune",
  "kind": "movie",             // movie | series | live | audio | other
  "poster": "https://…",       // optional
  "year": 2021,                // optional
  "subtitle": "Part One"       // optional
}
```

```jsonc
// StreamSource
{
  "url": "https://cdn…/master.m3u8",
  "quality": "fhd1080",        // uhd2160 | fhd1080 | hd720 | sd480 | low | auto | unknown
  "headers": [["Referer", "https://site/"], ["User-Agent", "…"]],
  "subtitles": [{ "url": "…", "language": "en", "label": "English", "default": true }],
  "label": "HLS · Cloudflare",
  "live": false
}
```

`headers` are replayed by the playback engine, which is how a provider passes
Referer/UA to a CDN that rejects direct requests.

```jsonc
// ProviderManifest
{
  "id": "demo",
  "name": "Demo Provider",
  "version": "1.0.0",
  "description": "Shows the ABI without shipping any content logic.",
  "language": "en",
  "capabilities": { "home": true, "search": true, "details": true, "resolve": true, "live": false }
}
```

`HomeRow`, `MediaDetails`, `Episode` and `SubtitleTrack` follow the same pattern;
see `crates/app-core/src/domain.rs`.

---

## 4. Host functions

Two imports are available. Anything else fails to instantiate — that failure is
intentional and is the sandbox boundary.

### `host_http_request(payload: String) -> String`

```jsonc
// input
{ "url": "https://site/search?q=dune", "method": "GET", "headers": [["Accept","text/html"]], "body": null }

// output
{ "status": 200, "headers": [["content-type","text/html"]], "body": "<html>…", "error": null }
```

* Only `http`/`https` are accepted.
* Bodies are capped at 8 MiB and returned as text (providers parse HTML/JSON, not
  binary).
* `status: 0` with `error` set means the transport failed.

### `host_log(line: String) -> String`

Writes one line to the application log. The return value is always `""`.

### Deliberately absent: `parse_html`

An earlier draft exposed an HTML selector engine on the host. It was removed:
pulling `html5ever` into the host costs ~2 MB of binary for something the guest
can do itself. Providers bundle a small parser (`tl`, ~50 KB compiled into the
module) and parse locally. This keeps the host's dependency surface — and the
attack surface — smaller.

---

## 5. Limits

| Limit | Value | Enforced by |
|---|---|---|
| Memory | 64 pages (4 MiB) | Extism `memory_max` |
| Call wall-clock | 30 s | Extism timeout |
| Host HTTP response | 8 MiB | host function |
| Repository index size | 4 MiB | `app-net` request cap |
| Extension binary | 16 MiB | `app-net` request cap |
| Provider id | `[A-Za-z0-9._-]{1,128}` | manifest validation |

A guest that exceeds a limit traps; the host converts that into
`ProviderError::Sandbox` and keeps the rest of the app working.

---

## 6. Repository index

```jsonc
{
  "name": "Example Repository",
  "description": "One demo provider.",
  "manifest_version": 1,
  "extensions": [
    {
      "provider_id": "demo",
      "name": "Demo Provider",
      "version": "1.0.0",
      "description": "…",
      "language": "en",
      "url": "https://…/demo.wasm",      // relative URLs resolve against this index
      "sha256": "<64 hex chars>",        // mandatory
      "icon_url": "https://…/demo.png",  // optional
      "size_bytes": 812344               // optional
    }
  ],
  "signature": {                          // optional, verified when present
    "public_key": "<64 hex chars>",
    "signature":  "<128 hex chars>"
  }
}
```

The signed payload is the canonical JSON of:

```json
{ "manifest_version": 1, "extensions": [ … ] }
```

so signing does not depend on the repository's display name.

`manifest_version` above 1 is refused rather than guessed at. Every entry needs a
valid SHA-256; an entry without one is rejected before any download starts.

---

## 7. Writing a provider

1. `cargo new --lib my-provider`
2. `Cargo.toml`:
   ```toml
   [lib]
   crate-type = ["cdylib"]
   [dependencies]
   extism-pdk = "1"
   serde = { version = "1", features = ["derive"] }
   ```
3. Implement the five exports, test them natively (they are just functions), then
   build for the guest:
   ```
   cargo build --release --target wasm32-unknown-unknown
   ```
4. Publish the `.wasm` plus its SHA-256 in a repository index.

See `plugins/example-provider/` for a complete, minimal module.

[Extism]: https://extism.org/docs/concepts/plug-in
