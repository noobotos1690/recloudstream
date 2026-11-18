//! The runtime: everything that is allowed to block.
//!
//! Structure:
//!
//! * **UI thread** (this crate's `App::update`) owns the state lock, calls the
//!   reducer, draws, and dispatches effects.
//! * **Work thread** consumes [`Effect`]s that involve the network or a
//!   provider, then posts [`Intent`]s back.
//! * **Provider threads** are spawned per search so one slow extension cannot
//!   delay the others.
//! * **Cover thread** downloads artwork into the cache directory; the UI only
//!   ever decodes files that already exist.
//!
//! Nothing here mutates [`AppState`]; results travel back as intents, so the
//! reducer stays the single writer.

use app_core::{
    AppState, Effect, HttpClient, HttpRequest, InstalledExtension, Intent, PlaybackCommand,
    ResolveOptions, Settings,
};
use app_net::UreqClient;
use app_plugin_host::{ExtensionInstaller, HttpRepository, LogSink, ProviderRegistry as Registry};
use app_storage::Store;
use app_ui::EngineHandle;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

/// Everything the work thread needs. Kept in one struct so the loop signature
/// stays readable.
struct WorkerDeps {
    http: Arc<dyn HttpClient>,
    installer: ExtensionInstaller,
    registry: Arc<RwLock<Registry>>,
    installed: Arc<RwLock<Vec<InstalledExtension>>>,
    /// Built-in providers the user switched off, mirrored for the worker.
    disabled_builtins: Arc<RwLock<Vec<String>>>,
    covers: Sender<Vec<String>>,
    log: LogSink,
}

pub struct Runtime {
    pub state: Arc<Mutex<AppState>>,
    /// Created on the UI thread once the OpenGL context exists.
    pub engine: Option<EngineHandle>,
    store: Store,
    effects: Sender<Effect>,
    results: Receiver<Intent>,
    installed: Arc<RwLock<Vec<InstalledExtension>>>,
    disabled_builtins: Arc<RwLock<Vec<String>>>,
}

impl Runtime {
    /// Open storage, restore the previous session and start the workers.
    pub fn boot() -> Result<Self, String> {
        let store = Store::open_default().map_err(|err| err.to_string())?;
        let snapshot = store.load_snapshot();

        let mut state = AppState::default();
        app_core::reduce(&mut state, Intent::Hydrate(Box::new(snapshot)));
        state.backend.data_dir = store.root().to_string_lossy().into_owned();
        state.backend.extension_dir = store.extensions_dir().to_string_lossy().into_owned();
        state.backend.wasm_runtime = true;

        // A profile that has never stored a repository list starts with the
        // built-in ones pre-added; the fetch below is what surfaces their state.
        let seed_builtins = !store.has_repositories();
        if seed_builtins {
            for (name, url) in app_core::BUILTIN_REPOSITORIES {
                state
                    .extensions
                    .repositories
                    .push(app_core::RepositoryRecord {
                        url: (*url).to_string(),
                        name: (*name).to_string(),
                        fetched_at: 0,
                        extension_count: 0,
                        last_error: None,
                    });
            }
        }

        let timeout = Duration::from_secs(state.settings.network_timeout_secs.clamp(5, 120));
        let http: Arc<dyn HttpClient> = Arc::new(UreqClient::new(timeout));
        let log = log_sink(&store);

        // Built-in providers are compiled in, so the list is known here rather
        // than in the core: the core only carries their view state.
        state.extensions.builtins = app_providers::builtins()
            .iter()
            .map(|builtin| app_core::BuiltinView {
                id: builtin.id.to_string(),
                name: builtin.name.to_string(),
                version: builtin.version.to_string(),
                description: builtin.description.to_string(),
                origin: builtin.origin.to_string(),
                capabilities: builtin.capabilities.to_string(),
                enabled: !state
                    .settings
                    .disabled_builtins
                    .iter()
                    .any(|id| id == builtin.id),
                testing: false,
                last_test: None,
            })
            .collect();

        let installed = Arc::new(RwLock::new(state.extensions.installed.clone()));
        let registry = Arc::new(RwLock::new(Registry::empty()));
        let disabled_builtins = Arc::new(RwLock::new(state.settings.disabled_builtins.clone()));

        let (effect_tx, effect_rx) = channel();
        let (intent_tx, intent_rx) = channel();
        let (cover_tx, cover_rx) = channel();

        let covers_dir = store.cache_dir().join("covers");
        spawn_cover_worker(cover_rx, Arc::clone(&http), covers_dir);

        let deps = WorkerDeps {
            installer: ExtensionInstaller::new(Arc::clone(&http), store.clone()),
            http,
            registry: Arc::clone(&registry),
            installed: Arc::clone(&installed),
            disabled_builtins: Arc::clone(&disabled_builtins),
            covers: cover_tx,
            log: Arc::clone(&log),
        };
        spawn_work_worker(deps, effect_rx, intent_tx.clone());

        if seed_builtins {
            for (_, url) in app_core::BUILTIN_REPOSITORIES {
                let _ = effect_tx.send(Effect::FetchRepository {
                    url: (*url).to_string(),
                });
            }
        }

        // A repository that has never loaded is retried once per start, so an
        // offline moment or a moved host heals by itself. Seeded ones carry no
        // error yet and are not fetched twice.
        for url in state.extensions.repositories_to_refresh() {
            let _ = effect_tx.send(Effect::FetchRepository { url });
        }

        Ok(Self {
            state: Arc::new(Mutex::new(state)),
            engine: None,
            store,
            effects: effect_tx,
            results: intent_rx,
            installed,
            disabled_builtins,
        })
    }

    /// Results posted by the worker since the last frame.
    pub fn drain_results(&self) -> Vec<Intent> {
        self.results.try_iter().collect()
    }

    pub fn send(&self, effect: Effect) {
        let _ = self.effects.send(effect);
    }

    /// Mirror the installed-extension list for the worker; the worker needs it
    /// to rebuild the registry after an install or removal.
    pub fn set_installed(&self, extensions: Vec<InstalledExtension>) {
        if let Ok(mut guard) = self.installed.write() {
            *guard = extensions;
        }
    }

    /// Ask the worker to rebuild the provider registry (startup and after any
    /// extension change).
    pub fn reload_providers(&self) {
        self.send(Effect::ReloadProviders);
    }

    pub fn save_settings(&self, settings: &Settings) {
        if let Err(err) = self.store.save_settings(settings) {
            eprintln!("settings: {err}");
        }
        // The worker rebuilds the registry from this list, so it has to see the
        // new exclusions before the ReloadProviders effect that follows.
        if let Ok(mut guard) = self.disabled_builtins.write() {
            *guard = settings.disabled_builtins.clone();
        }
    }

    pub fn save_bookmarks(&self, state: &AppState) {
        if let Err(err) = self.store.save_bookmarks(&state.library.bookmarks) {
            eprintln!("bookmarks: {err}");
        }
    }

    pub fn save_progress(&self, state: &AppState) {
        if let Err(err) = self.store.save_progress(&state.library.progress) {
            eprintln!("progress: {err}");
        }
    }

    pub fn save_repositories(&self, state: &AppState) {
        if let Err(err) = self.store.save_repositories(&state.extensions.repositories) {
            eprintln!("repositories: {err}");
        }
    }

    pub fn save_extensions(&self, state: &AppState) {
        let extensions = state.extensions.installed.clone();
        if let Err(err) = self.store.save_extensions(&extensions) {
            eprintln!("extensions: {err}");
        }
        self.set_installed(extensions);
    }

    pub fn send_player_command(&self, command: PlaybackCommand) {
        if let Some(engine) = &self.engine {
            if let Ok(mut engine) = engine.lock() {
                let _ = engine.command(command);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Work thread
// ---------------------------------------------------------------------------

fn spawn_work_worker(deps: WorkerDeps, effects: Receiver<Effect>, intents: Sender<Intent>) {
    std::thread::Builder::new()
        .name("app-work".to_string())
        .spawn(move || {
            while let Ok(effect) = effects.recv() {
                handle_effect(effect, &deps, &intents);
            }
        })
        .expect("cannot start the work thread");
}

fn handle_effect(effect: Effect, deps: &WorkerDeps, intents: &Sender<Intent>) {
    match effect {
        Effect::LoadHome => {
            let registry = deps
                .registry
                .read()
                .map(|registry| registry.all().to_vec())
                .unwrap_or_default();
            let mut rows = Vec::new();
            let mut errors = Vec::new();
            for provider in registry {
                if !provider.manifest().capabilities.home {
                    continue;
                }
                match provider.home() {
                    Ok(mut home) => rows.append(&mut home),
                    Err(err) => errors.push(format!("{}: {err}", provider.manifest().name)),
                }
            }
            let posters = collect_posters(&rows);
            let _ = deps.covers.send(posters);
            if rows.is_empty() && !errors.is_empty() {
                let _ = intents.send(Intent::HomeFailed(errors.join("; ")));
            } else {
                let _ = intents.send(Intent::HomeLoaded(rows));
            }
        }

        Effect::Search {
            query,
            provider_ids,
        } => {
            for provider_id in provider_ids {
                let Some(provider) = deps
                    .registry
                    .read()
                    .ok()
                    .and_then(|registry| registry.get(&provider_id))
                else {
                    continue;
                };
                let intents = intents.clone();
                let covers = deps.covers.clone();
                let query = query.clone();
                // One thread per provider: a slow extension must not delay the
                // others, and the orchestrator contract allows unbounded fan-out
                // because the provider count is small (one per installed plugin).
                std::thread::Builder::new()
                    .name(format!("search-{provider_id}"))
                    .spawn(move || match provider.search(&query) {
                        Ok(items) => {
                            let _ = covers
                                .send(items.iter().filter_map(|i| i.poster.clone()).collect());
                            let _ = intents.send(Intent::SearchResults {
                                provider_id,
                                query,
                                items,
                            });
                        }
                        Err(err) => {
                            let _ = intents.send(Intent::SearchFailed {
                                provider_id,
                                query,
                                error: err.to_string(),
                            });
                        }
                    })
                    .ok();
            }
        }

        Effect::LoadDetails { item } => {
            let provider = deps
                .registry
                .read()
                .ok()
                .and_then(|registry| registry.get(&item.provider_id));
            match provider {
                None => {
                    let _ = intents.send(Intent::DetailsFailed(format!(
                        "provider `{}` is not loaded",
                        item.provider_id
                    )));
                }
                Some(provider) => match provider.details(&item.id) {
                    Ok(details) => {
                        if let Some(poster) = details.item.poster.clone() {
                            let _ = deps.covers.send(vec![poster]);
                        }
                        let _ = intents.send(Intent::DetailsLoaded(Box::new(details)));
                    }
                    Err(err) => {
                        let _ = intents.send(Intent::DetailsFailed(err.to_string()));
                    }
                },
            }
        }

        Effect::ResolveSources { item, episode_id } => {
            let provider = deps
                .registry
                .read()
                .ok()
                .and_then(|registry| registry.get(&item.provider_id));
            match provider {
                None => {
                    let _ = intents.send(Intent::SourcesFailed(format!(
                        "provider `{}` is not loaded",
                        item.provider_id
                    )));
                }
                Some(provider) => {
                    let options = ResolveOptions {
                        episode_id,
                        preferred_quality: app_core::Quality::Auto,
                    };
                    match provider.resolve(&item.id, &options) {
                        Ok(sources) => {
                            let _ = intents.send(Intent::SourcesResolved {
                                item: Box::new(item),
                                sources,
                            });
                        }
                        Err(err) => {
                            let _ = intents.send(Intent::SourcesFailed(err.to_string()));
                        }
                    }
                }
            }
        }

        Effect::FetchRepository { url } => {
            let repository = HttpRepository::new(url.clone(), Arc::clone(&deps.http));
            use app_core::RepositorySource;
            match repository.fetch() {
                Ok(fetched) => {
                    let _ = intents.send(Intent::RepositoryFetched {
                        url,
                        repository: Box::new(fetched),
                    });
                }
                Err(err) => {
                    let _ = intents.send(Intent::RepositoryFailed {
                        url,
                        error: err.to_string(),
                    });
                }
            }
        }

        Effect::InstallExtension { repository, entry } => {
            match deps.installer.install(&repository, &entry) {
                Ok(installed) => {
                    let _ = intents.send(Intent::ExtensionInstalled(Box::new(installed)));
                }
                Err(err) => {
                    let _ = intents.send(Intent::ExtensionFailed {
                        provider_id: entry.provider_id.clone(),
                        error: err.to_string(),
                    });
                }
            }
        }

        Effect::DeleteExtensionFile { wasm_path } => {
            if let Err(err) = deps.installer.remove(&wasm_path) {
                (deps.log)(format!("remove extension: {err}"));
            }
        }

        Effect::ReloadProviders => {
            let installed = deps
                .installed
                .read()
                .map(|list| list.clone())
                .unwrap_or_default();
            let mut registry =
                Registry::load(&installed, Arc::clone(&deps.http), Arc::clone(&deps.log));
            let disabled = deps
                .disabled_builtins
                .read()
                .map(|list| list.clone())
                .unwrap_or_default();
            for builtin in app_providers::builtins() {
                if disabled.iter().any(|id| id == builtin.id) {
                    continue;
                }
                if let Some(provider) = app_providers::create(builtin.id, Arc::clone(&deps.http)) {
                    registry.register(provider);
                }
            }
            for (provider_id, error) in &registry.failures {
                (deps.log)(format!("extension `{provider_id}` failed to load: {error}"));
            }
            let manifests = registry.manifests();
            if let Ok(mut guard) = deps.registry.write() {
                *guard = registry;
            }
            let _ = intents.send(Intent::ProvidersRefreshed(manifests));
        }

        Effect::TestBuiltin(id) => {
            let provider = deps
                .registry
                .read()
                .ok()
                .and_then(|registry| registry.get(&id));
            let Some(provider) = provider else {
                let _ = intents.send(Intent::BuiltinTested {
                    id,
                    ok: false,
                    detail: "provider is disabled or failed to load".to_string(),
                });
                return;
            };
            let started = std::time::Instant::now();
            let outcome = provider.search("avatar");
            let elapsed = started.elapsed().as_millis();
            let (ok, detail) = match outcome {
                Ok(items) => (true, format!("{} result(s) in {elapsed} ms", items.len())),
                Err(err) => (false, format!("{err} ({elapsed} ms)")),
            };
            let _ = intents.send(Intent::BuiltinTested { id, ok, detail });
        }

        // Persistence and player effects are handled by the UI thread; they are
        // matched here so the compiler keeps this list exhaustive.
        Effect::PersistSettings
        | Effect::PersistBookmarks
        | Effect::PersistProgress
        | Effect::PersistRepositories
        | Effect::PersistExtensions
        | Effect::PlayerLoad { .. }
        | Effect::PlayerCommand(_)
        | Effect::PlayerStop => {}
    }
}

fn collect_posters(rows: &[app_core::HomeRow]) -> Vec<String> {
    rows.iter()
        .flat_map(|row| row.items.iter())
        .filter_map(|item| item.poster.clone())
        .collect()
}

// ---------------------------------------------------------------------------
// Cover thread
// ---------------------------------------------------------------------------

fn spawn_cover_worker(urls: Receiver<Vec<String>>, http: Arc<dyn HttpClient>, dir: PathBuf) {
    std::thread::Builder::new()
        .name("covers".to_string())
        .spawn(move || {
            while let Ok(batch) = urls.recv() {
                for url in batch {
                    if !(url.starts_with("http://") || url.starts_with("https://")) {
                        continue;
                    }
                    let path = app_ui::CoverCache::path_for(&dir, &url);
                    if path.exists() {
                        continue;
                    }
                    let mut request = HttpRequest::get(url.clone());
                    request.max_bytes = 3 * 1024 * 1024;
                    match http.execute(request) {
                        Ok(response) if response.is_success() => {
                            if let Some(parent) = path.parent() {
                                let _ = std::fs::create_dir_all(parent);
                            }
                            if let Err(err) = std::fs::write(&path, response.body) {
                                eprintln!("cover write: {err}");
                            }
                        }
                        Ok(response) => {
                            eprintln!("cover fetch: {url} returned HTTP {}", response.status)
                        }
                        Err(err) => eprintln!("cover fetch: {err}"),
                    }
                }
            }
        })
        .expect("cannot start the cover thread");
}

// ---------------------------------------------------------------------------
// Logging
// ---------------------------------------------------------------------------

fn log_sink(store: &Store) -> LogSink {
    let path = store.root().join("app.log");
    Arc::new(move |line: String| {
        use std::io::Write;
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            let _ = writeln!(file, "{line}");
        }
    })
}
