//! Cover-art cache.
//!
//! The UI never performs network I/O. The runtime downloads cover bytes into
//! `%LOCALAPPDATA%\CloudStreamDesktop\cache\covers\<hash>`; this module only
//! decodes a file that already exists and uploads it as a texture, once.
//!
//! That split matters for two reasons: a slow image CDN cannot stall a frame, and
//! the UI crate stays free of the HTTP stack.

use crate::theme::stable_hash;
use egui::{ColorImage, Context, TextureHandle, TextureOptions};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

enum Entry {
    Ready(TextureHandle),
    /// Looked for and not found; not retried this session.
    Missing,
}

#[derive(Default)]
pub struct CoverCache {
    entries: HashMap<String, Entry>,
    dir: Option<PathBuf>,
}

impl CoverCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_dir(&mut self, dir: PathBuf) {
        if self.dir.as_ref() != Some(&dir) {
            self.dir = Some(dir);
            self.entries.clear();
        }
    }

    /// Absolute path the runtime writes a cover to.
    pub fn path_for(dir: &Path, url: &str) -> PathBuf {
        dir.join(format!("{:016x}", stable_hash(url)))
    }

    pub fn get(&mut self, ctx: &Context, url: &str) -> Option<TextureHandle> {
        if let Some(Entry::Ready(handle)) = self.entries.get(url) {
            return Some(handle.clone());
        }
        if matches!(self.entries.get(url), Some(Entry::Missing)) {
            return None;
        }

        let dir = self.dir.clone()?;
        let path = Self::path_for(&dir, url);
        let handle = load_texture(ctx, &path);
        match handle {
            Some(handle) => {
                self.entries
                    .insert(url.to_string(), Entry::Ready(handle.clone()));
                Some(handle)
            }
            None => {
                self.entries.insert(url.to_string(), Entry::Missing);
                None
            }
        }
    }

    /// Drop decoded textures (used when the cache directory is cleared).
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

fn load_texture(ctx: &Context, path: &Path) -> Option<TextureHandle> {
    // Cover files are written by the runtime, not the network, and are capped in
    // size there; reading synchronously costs one small file per tile.
    let bytes = std::fs::read(path).ok()?;
    let decoded = image::load_from_memory(&bytes).ok()?;
    let rgba = decoded.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    if size[0] == 0 || size[1] == 0 {
        return None;
    }
    let image = ColorImage::from_rgba_unmultiplied(size, &rgba.into_raw());
    Some(ctx.load_texture(
        format!("cover-{}", stable_hash(&path.to_string_lossy())),
        image,
        TextureOptions::LINEAR,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cover_paths_are_stable_per_url() {
        let dir = Path::new("C:/cache/covers");
        let a = CoverCache::path_for(dir, "https://img.test/a.jpg");
        let b = CoverCache::path_for(dir, "https://img.test/a.jpg");
        let c = CoverCache::path_for(dir, "https://img.test/b.jpg");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert!(a.starts_with(dir));
    }
}
