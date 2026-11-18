//! Extension host: repository indexes, install/verify, and the WASM sandbox.
//!
//! The application ships with **no content and no providers**. Everything the
//! user watches comes from an extension they installed themselves, which is the
//! same philosophy CloudStream uses on Android — with a stricter execution model.
//!
//! * Android loads provider code as a JAR with the app's full privileges.
//! * This host loads provider code as a WebAssembly module with no filesystem,
//!   no sockets and a fixed allowance of host functions.
//!
//! See `docs/PLUGIN_ABI.md` for the wire format and `docs/ARCHITECTURE.md` for
//! the trust model.

pub mod installer;
pub mod manifest;
pub mod provider;
pub mod registry;
pub mod repo;

pub use installer::{hex_sha256, ExtensionInstaller};
pub use manifest::{parse_repository, trust_level, validate_entry, validate_repository, Trust};
pub use provider::WasmProvider;
pub use registry::{LogSink, ProviderRegistry};
pub use repo::{HttpRepository, MAX_EXTENSION_BYTES};
