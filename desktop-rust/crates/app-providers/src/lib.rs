//! Built-in providers: content sources compiled into the application.
//!
//! These exist because the CloudStream Android repository this project shipped
//! with publishes `.cs3` bundles — Android bytecode that a Windows build cannot
//! load. Rewriting a provider in Rust is the only way to make one of those
//! sources work here, so this crate holds those rewrites.
//!
//! A built-in provider is an ordinary `app_core::Provider`; the registry treats
//! it exactly like a WebAssembly extension. That means Home, Search, Details and
//! Play needed no changes to support them.

mod ctgmovies;
mod html;

pub use ctgmovies::CtgMovies;

/// A provider that ships inside the binary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuiltinProvider {
    pub id: &'static str,
    pub name: &'static str,
    pub version: &'static str,
    pub description: &'static str,
    /// Where the source lives, so the UI can say what a click will contact.
    pub origin: &'static str,
    /// Short summary of what the provider implements, shown as a chip.
    pub capabilities: &'static str,
}

pub const CTGMOVIES_ID: &str = "builtin-ctgmovies";

static BUILTINS: &[BuiltinProvider] = &[BuiltinProvider {
    id: CTGMOVIES_ID,
    name: "CTGMovies",
    version: "1.0.0",
    description: "Films, series and anime, with direct file links for playback.",
    origin: "https://ctgmovies.com",
    capabilities: "home · search · details · sources",
}];

/// Every built-in provider this build knows about.
pub fn builtins() -> &'static [BuiltinProvider] {
    BUILTINS
}

pub fn find(id: &str) -> Option<&'static BuiltinProvider> {
    BUILTINS.iter().find(|builtin| builtin.id == id)
}

/// Instantiate a built-in provider, or `None` for an unknown id.
pub fn create(
    id: &str,
    http: std::sync::Arc<dyn app_core::HttpClient>,
) -> Option<std::sync::Arc<dyn app_core::Provider>> {
    match id {
        CTGMOVIES_ID => Some(std::sync::Arc::new(CtgMovies::new(http))),
        _ => None,
    }
}
