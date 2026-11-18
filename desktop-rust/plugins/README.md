# Writing a desktop extension

CloudStream Desktop runs **WebAssembly** extensions, not Android `.cs3` APKs. A
`.cs3` is a Kotlin/DEX package loaded into an Android process; a `.wasm` is a
sandboxed module that can only call the two host functions listed in
[`../docs/PLUGIN_ABI.md`](../docs/PLUGIN_ABI.md). Porting a provider therefore
means reimplementing its HTTP + parsing logic, not repackaging the APK.

## Why these crates live outside the workspace

`desktop-rust/Cargo.toml` lists each plugin under `exclude`. An extension
registers Extism *host* functions (`host_http_request`, `host_log`) that exist
only inside the sandbox, so a plugin crate must be built for
`wasm32-unknown-unknown` and never pulled into a host build. Building the
workspace must not try to link them.

## Build

```powershell
rustup target add wasm32-unknown-unknown
cargo build --release --target wasm32-unknown-unknown `
  --manifest-path plugins\ctgmovies\Cargo.toml
```

The artifact is
`plugins\ctgmovies\target\wasm32-unknown-unknown\release\ctgmovies_plugin.wasm`.

## How `ctgmovies` reuses the built-in provider

`plugins/ctgmovies` does not copy the scraping code. It depends on
`app-providers` and constructs the same `CtgMovies` the binary ships with,
supplying an `HttpClient` that forwards to `host_http_request`. One
implementation of the markup rules therefore backs both the built-in provider and
the downloadable extension, and they cannot drift apart.

Two things are still required of any guest:

* **Its own `provider_id`.** `ctgmovies` reports `ctgmovies`, while the built-in
  reports `builtin-ctgmovies`. The registry keys providers by id and replaces on
  collision, so reusing the built-in id would silently shadow it. Every returned
  `MediaItem` must carry the guest's id too, which is why the guest re-tags them.
* **`provider_manifest` returns a bare manifest**, with no `Envelope`. The host
  parses it before anything else runs and refuses the plugin on failure.

## Publish

1. Add the entry to the index that ships with a release
   (`releases/<tag>/repo.json`), with the binary's **bare 64-character** SHA-256 —
   not Android's `sha256-<hex>` form.
2. Run `pwsh -File scripts\release.ps1 -Tag <tag>`. It stages the built binary
   under the name the index uses and uploads installer, index and binary together.
3. The index is then served from
   `https://github.com/<owner>/<repo>/releases/latest/download/repo.json`.

Entry URLs may be absolute or relative to the index. A relative entry such as
`"url": "ctgmovies.wasm"` is joined onto the index's own https origin by
`HttpRepository::resolve`.
