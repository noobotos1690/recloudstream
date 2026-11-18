//! `--selftest`: exercise every subsystem that does not need a window.
//!
//! This is what makes the project verifiable on a machine with no GPU and no
//! libmpv. It boots the same code the app boots, in a throwaway data directory,
//! and reports pass/fail per check with a non-zero exit code on failure.

use app_core::{
    AppState, Bookmark, Breakpoint, HttpClient, HttpRequest, HttpResponse, Intent, LayoutMetrics,
    MediaItem, PlaybackCommand, ProviderError, Route, Settings,
};
use app_playback::create_engine;
use app_plugin_host::{parse_repository, trust_level, ProviderRegistry, Trust};
use app_storage::Store;

struct Report {
    checks: usize,
    failures: usize,
    /// Collected rather than printed: a release build is a window application
    /// with no console of its own, so `main` decides where the text goes.
    lines: Vec<String>,
}

impl Report {
    fn new() -> Self {
        Self {
            checks: 0,
            failures: 0,
            lines: Vec::new(),
        }
    }

    fn check(&mut self, name: &str, passed: bool, detail: impl AsRef<str>) {
        self.checks += 1;
        if !passed {
            self.failures += 1;
        }
        let detail = detail.as_ref();
        let suffix = if detail.is_empty() {
            String::new()
        } else {
            format!(" — {detail}")
        };
        self.lines.push(format!(
            "[{}] {name}{suffix}",
            if passed { "pass" } else { "FAIL" }
        ));
    }
}

pub fn run() -> (i32, String) {
    let mut report = Report::new();

    // -- storage -----------------------------------------------------------
    let dir = std::env::temp_dir().join(format!(
        "cloudstream-desktop-selftest-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    match Store::open(dir.clone()) {
        Ok(store) => {
            let settings = Settings {
                cache_limit_mb: 256,
                ..Settings::default()
            };
            let saved = store.save_settings(&settings).is_ok();
            let bookmarks = vec![Bookmark::new(MediaItem::new("demo", "1", "Test"), 0)];
            let bookmarks_saved = store.save_bookmarks(&bookmarks).is_ok();
            let reloaded = store.load_snapshot();
            report.check(
                "storage: settings persist",
                saved && reloaded.settings.cache_limit_mb == 256,
                dir.display().to_string(),
            );
            report.check(
                "storage: bookmarks persist",
                bookmarks_saved && reloaded.bookmarks.len() == 1,
                "one bookmark round-tripped",
            );
        }
        Err(err) => report.check("storage: open data directory", false, err.to_string()),
    }

    // -- reducer -----------------------------------------------------------
    let mut state = AppState::default();
    let effects = app_core::reduce(&mut state, Intent::Navigate(Route::Extensions));
    report.check(
        "reducer: navigation",
        state.route == Route::Extensions && effects.is_empty(),
        "route changed without side effects",
    );

    state.browse.providers.clear();
    let effects = app_core::reduce(&mut state, Intent::SearchSubmit("dune".into()));
    report.check(
        "reducer: search with no providers is inert",
        effects.is_empty() && state.toasts.len() == 1,
        "explains itself instead of failing silently",
    );

    let before = state.library.bookmarks.len();
    app_core::reduce(
        &mut state,
        Intent::ToggleBookmark(Box::new(MediaItem::new("demo", "42", "Dune"))),
    );
    report.check(
        "reducer: bookmark toggle",
        state.library.bookmarks.len() == before + 1,
        "added",
    );

    // -- layout ------------------------------------------------------------
    let compact = LayoutMetrics::compute(600.0, 900.0, 1.0, None);
    let expanded = LayoutMetrics::compute(1800.0, 1000.0, 1.0, None);
    report.check(
        "layout: breakpoints",
        compact.breakpoint == Breakpoint::Compact
            && expanded.breakpoint == Breakpoint::Expanded
            && compact.rail_width == 0.0,
        format!(
            "compact {} cols, expanded {} cols",
            compact.columns, expanded.columns
        ),
    );
    let mut grid_ok = true;
    for width in [420.0_f32, 768.0, 1280.0, 2560.0] {
        let metrics = LayoutMetrics::compute(width, 900.0, 1.0, None);
        let used = metrics.card_width * metrics.columns as f32
            + metrics.gutter * (metrics.columns.saturating_sub(1)) as f32;
        if used > metrics.content_width(width) + 1.0 {
            grid_ok = false;
        }
    }
    report.check("layout: grid never overflows", grid_ok, "420–2560 px");

    // -- playback ----------------------------------------------------------
    let (mut engine, engine_report) = create_engine();
    let load_ok = engine
        .load(app_core::PlaybackRequest {
            url: "https://example.test/stream.m3u8".into(),
            headers: vec![],
            subtitles: vec![],
            title: "Self test".into(),
            start_position: 0.0,
            live: false,
        })
        .is_ok();
    let pause_ok = engine.command(PlaybackCommand::Pause).is_ok();
    let events = engine.poll();
    engine.shutdown();
    report.check(
        "playback: engine initialises",
        load_ok && pause_ok,
        engine_report.detail,
    );
    report.check(
        "playback: timeline emits events",
        !events.is_empty(),
        format!("{} event(s)", events.len()),
    );

    // -- plugin host -------------------------------------------------------
    let index = r#"{
        "name": "Self test repo",
        "description": "Parsed at startup to prove the index format still reads.",
        "manifest_version": 1,
        "extensions": [{
            "provider_id": "selftest",
            "name": "Self test",
            "version": "1.0.0",
            "url": "https://example.test/selftest.wasm",
            "sha256": "0000000000000000000000000000000000000000000000000000000000000000"
        }]
    }"#;
    match parse_repository(index) {
        Ok(repository) => {
            let trust = trust_level(&repository);
            report.check(
                "plugins: repository index parses",
                repository.extensions.len() == 1,
                format!("{} extension(s)", repository.extensions.len()),
            );
            report.check(
                "plugins: unsigned index reported as untrusted",
                matches!(trust, Ok(Trust::Unsigned)),
                "hash pinning still applies",
            );
        }
        Err(err) => report.check("plugins: repository index parses", false, err.to_string()),
    }

    // -- built-in providers ------------------------------------------------
    // No network here: this proves the wiring (a built-in registers in the same
    // registry as an extension and is reachable by id), not that the site is up.
    struct NoHttp;
    impl HttpClient for NoHttp {
        fn execute(&self, _request: HttpRequest) -> Result<HttpResponse, ProviderError> {
            Err(ProviderError::Sandbox("self-test has no network".into()))
        }
    }

    let specs = app_providers::builtins();
    let mut registry = ProviderRegistry::empty();
    let mut registered = 0;
    for spec in specs {
        if let Some(provider) = app_providers::create(spec.id, std::sync::Arc::new(NoHttp)) {
            let manifest = provider.manifest();
            if manifest.id == spec.id && manifest.capabilities.search {
                registry.register(provider);
                registered += 1;
            }
        }
    }
    let reachable = specs
        .first()
        .map(|spec| registry.get(spec.id).is_some())
        .unwrap_or(false);
    report.check(
        "providers: built-ins register like extensions",
        registered == specs.len() && reachable,
        format!("{registered} built-in provider(s)"),
    );

    let _ = std::fs::remove_dir_all(&dir);

    let mut out = String::from("CloudStream Desktop self-test\n\n");
    for line in &report.lines {
        out.push_str(line);
        out.push('\n');
    }
    out.push_str(&format!(
        "\n{} check(s), {} failure(s)\n",
        report.checks, report.failures
    ));
    if report.failures == 0 {
        out.push_str("OK\n");
        (0, out)
    } else {
        out.push_str("FAILED\n");
        (1, out)
    }
}
