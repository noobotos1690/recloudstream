//! Timeline-only playback engine.
//!
//! It runs the same state machine a real decoder would — load, play, pause,
//! seek, end — but emits no frames. The UI can therefore be developed, tested
//! and demoed without libmpv, and the player controls still do something
//! visible.

use app_core::{
    EngineKind, PlaybackCommand, PlaybackEngine, PlaybackError, PlaybackEvent, PlaybackRequest,
    RenderTarget,
};
use std::time::Instant;

#[derive(Debug)]
pub struct NullEngine {
    loaded: bool,
    playing: bool,
    position: f64,
    duration: f64,
    speed: f64,
    volume: f64,
    muted: bool,
    last_tick: Instant,
    pending: Vec<PlaybackEvent>,
}

impl NullEngine {
    pub fn new() -> Self {
        Self {
            loaded: false,
            playing: false,
            position: 0.0,
            duration: 0.0,
            speed: 1.0,
            volume: 1.0,
            muted: false,
            last_tick: Instant::now(),
            pending: Vec::new(),
        }
    }
}

impl Default for NullEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl PlaybackEngine for NullEngine {
    fn kind(&self) -> EngineKind {
        EngineKind::Null
    }

    fn load(&mut self, request: PlaybackRequest) -> Result<(), PlaybackError> {
        self.loaded = true;
        self.playing = true;
        self.position = request.start_position.max(0.0);
        self.duration = 0.0;
        self.last_tick = Instant::now();
        self.pending.push(PlaybackEvent::Loaded { duration: None });
        self.pending.push(PlaybackEvent::Paused(false));
        Ok(())
    }

    fn command(&mut self, command: PlaybackCommand) -> Result<(), PlaybackError> {
        match command {
            PlaybackCommand::Play => {
                self.playing = true;
                self.pending.push(PlaybackEvent::Paused(false));
            }
            PlaybackCommand::Pause => {
                self.playing = false;
                self.pending.push(PlaybackEvent::Paused(true));
            }
            PlaybackCommand::TogglePause => {
                self.playing = !self.playing;
                self.pending.push(PlaybackEvent::Paused(!self.playing));
            }
            PlaybackCommand::SeekTo(target) => {
                self.position = target.max(0.0);
                self.pending.push(PlaybackEvent::Position {
                    position: self.position,
                    duration: self.optional_duration(),
                });
            }
            PlaybackCommand::SeekRelative(delta) => {
                self.position = (self.position + delta).max(0.0);
                self.pending.push(PlaybackEvent::Position {
                    position: self.position,
                    duration: self.optional_duration(),
                });
            }
            PlaybackCommand::SetVolume(volume) => self.volume = volume.clamp(0.0, 1.0),
            PlaybackCommand::SetMuted(muted) => self.muted = muted,
            PlaybackCommand::SetSpeed(speed) => self.speed = speed.clamp(0.1, 4.0),
            PlaybackCommand::SetSubtitle(_)
            | PlaybackCommand::SetAudioTrack(_)
            | PlaybackCommand::SetAspect(_) => {}
            PlaybackCommand::Stop => {
                self.loaded = false;
                self.playing = false;
                self.position = 0.0;
            }
        }
        Ok(())
    }

    fn poll(&mut self) -> Vec<PlaybackEvent> {
        let now = Instant::now();
        let delta = now.saturating_duration_since(self.last_tick).as_secs_f64();
        self.last_tick = now;

        if self.loaded && self.playing {
            self.position += delta * self.speed;
            self.pending.push(PlaybackEvent::Position {
                position: self.position,
                duration: self.optional_duration(),
            });
        }

        std::mem::take(&mut self.pending)
    }

    fn needs_render(&self) -> bool {
        false
    }

    fn render(&mut self, _target: RenderTarget) -> Result<(), PlaybackError> {
        // Nothing to composite: the UI draws its own "no decoder" placeholder.
        Ok(())
    }

    fn shutdown(&mut self) {
        self.loaded = false;
        self.playing = false;
    }
}

impl NullEngine {
    fn optional_duration(&self) -> Option<f64> {
        (self.duration > 0.0).then_some(self.duration)
    }

    /// Exposed for tests and the settings screen's diagnostics.
    pub fn is_playing(&self) -> bool {
        self.playing
    }

    pub fn volume(&self) -> f64 {
        self.volume
    }

    pub fn muted(&self) -> bool {
        self.muted
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> PlaybackRequest {
        PlaybackRequest {
            url: "https://example.test/stream.m3u8".into(),
            headers: vec![],
            subtitles: vec![],
            title: "Test".into(),
            start_position: 12.0,
            live: false,
        }
    }

    #[test]
    fn load_reports_ready_and_honours_resume_offset() {
        let mut engine = NullEngine::new();
        engine.load(request()).unwrap();
        let events = engine.poll();
        assert!(events
            .iter()
            .any(|e| matches!(e, PlaybackEvent::Loaded { .. })));
        // The first tick also advances by the (tiny) elapsed time, so assert a
        // window rather than exact equality.
        assert!(events.iter().any(|e| matches!(
            e,
            PlaybackEvent::Position { position, .. } if (12.0..12.1).contains(position)
        )));
        assert!(engine.is_playing());
    }

    #[test]
    fn pause_stops_advancing() {
        let mut engine = NullEngine::new();
        engine.load(request()).unwrap();
        engine.command(PlaybackCommand::Pause).unwrap();
        engine.poll();
        let events = engine.poll();
        assert!(events
            .iter()
            .all(|e| !matches!(e, PlaybackEvent::Position { .. })));
        assert!(!engine.is_playing());
    }

    #[test]
    fn seek_relative_never_goes_negative() {
        let mut engine = NullEngine::new();
        engine.load(request()).unwrap();
        engine
            .command(PlaybackCommand::SeekRelative(-999.0))
            .unwrap();
        let events = engine.poll();
        let position = events
            .iter()
            .find_map(|e| match e {
                PlaybackEvent::Position { position, .. } => Some(*position),
                _ => None,
            })
            .unwrap();
        assert_eq!(position, 0.0);
    }

    #[test]
    fn volume_is_clamped_and_mute_tracked() {
        let mut engine = NullEngine::new();
        engine.command(PlaybackCommand::SetVolume(3.0)).unwrap();
        assert_eq!(engine.volume(), 1.0);
        engine.command(PlaybackCommand::SetMuted(true)).unwrap();
        assert!(engine.muted());
    }
}
