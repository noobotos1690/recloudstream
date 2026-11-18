//! Playback engines.
//!
//! Two implementations exist behind one trait:
//!
//! * [`NullEngine`] — a timeline-only stub. Always available, never decodes.
//!   It exists so the application is fully usable on machines with no libmpv
//!   and so CI can drive the player state machine without a GPU.
//! * `MpvEngine` (feature `mpv`) — real decoding through `libmpv-2.dll`, loaded
//!   with `libloading` at runtime so a missing DLL degrades gracefully instead
//!   of failing to start the process.
//!
//! Both render into the framebuffer handed over by the UI, so the video is
//! composited by the same OpenGL context as the interface. libmpv resolves its
//! own GL entry points through WGL, which is why this crate needs nothing from
//! the UI beyond a target framebuffer.

mod null;

#[cfg(all(windows, feature = "mpv"))]
mod mpv;

use app_core::{EngineKind, PlaybackEngine};

pub use null::NullEngine;

#[cfg(all(windows, feature = "mpv"))]
pub use mpv::MpvEngine;

/// What the host actually managed to initialise, surfaced in Settings so a
/// missing DLL is a visible fact rather than a mystery.
#[derive(Debug, Clone)]
pub struct EngineReport {
    pub kind: EngineKind,
    pub detail: String,
}

#[cfg(all(windows, feature = "mpv"))]
pub fn create_engine() -> (Box<dyn PlaybackEngine>, EngineReport) {
    match MpvEngine::new() {
        Ok(engine) => {
            let detail = engine.describe();
            (
                Box::new(engine),
                EngineReport {
                    kind: EngineKind::LibMpv,
                    detail,
                },
            )
        }
        Err(err) => (
            Box::new(NullEngine::new()),
            EngineReport {
                kind: EngineKind::Null,
                detail: format!("libmpv unavailable ({err}); timeline-only playback"),
            },
        ),
    }
}

#[cfg(not(all(windows, feature = "mpv")))]
pub fn create_engine() -> (Box<dyn PlaybackEngine>, EngineReport) {
    (
        Box::new(NullEngine::new()),
        EngineReport {
            kind: EngineKind::Null,
            detail: "built without the `mpv` feature; timeline-only playback".to_string(),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use app_core::PlaybackCommand;

    #[test]
    fn engine_falls_back_instead_of_failing() {
        let (mut engine, report) = create_engine();
        // Either libmpv is present (rare on CI) or we degrade to the timeline.
        assert!(matches!(
            engine.kind(),
            EngineKind::LibMpv | EngineKind::Null
        ));
        assert!(!report.detail.is_empty());
        let _ = engine.command(PlaybackCommand::Pause);
        engine.shutdown();
    }

    #[test]
    fn reported_kind_matches_the_engine() {
        let (engine, report) = create_engine();
        assert_eq!(engine.kind(), report.kind);
    }
}
