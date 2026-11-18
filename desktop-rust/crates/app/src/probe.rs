//! `--probe <provider-id>`: prove a built-in provider still works.
//!
//! The Test button in the Extensions screen does the same thing in the UI. This
//! mode exists so the check can run in a terminal, on a build machine, or while
//! developing a provider, without opening a window.

use app_core::{HttpClient, ResolveOptions};
use app_net::UreqClient;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Returns the exit code and the report. Never panics on a network failure: a
/// broken site is a result to report, not a crash.
pub fn run(id: &str) -> (i32, String) {
    let mut out = String::new();
    let http: Arc<dyn HttpClient> = Arc::new(UreqClient::new(Duration::from_secs(30)));

    let Some(provider) = app_providers::create(id, Arc::clone(&http)) else {
        let known: Vec<&str> = app_providers::builtins()
            .iter()
            .map(|builtin| builtin.id)
            .collect();
        return (
            2,
            format!(
                "unknown built-in provider `{id}`\nknown: {}\n",
                known.join(", ")
            ),
        );
    };

    let manifest = provider.manifest();
    out.push_str(&format!("probe: {} ({})\n", manifest.name, manifest.id));

    let mut failures = 0;

    let started = Instant::now();
    match provider.home() {
        Ok(rows) => {
            let items: usize = rows.iter().map(|row| row.items.len()).sum();
            out.push_str(&format!(
                "[ok]   home: {} row(s), {items} item(s) in {} ms\n",
                rows.len(),
                started.elapsed().as_millis()
            ));
        }
        Err(err) => {
            failures += 1;
            out.push_str(&format!("[FAIL] home: {err}\n"));
        }
    }

    let started = Instant::now();
    match provider.search("avatar") {
        Ok(items) => {
            out.push_str(&format!(
                "[ok]   search \"avatar\": {} item(s) in {} ms\n",
                items.len(),
                started.elapsed().as_millis()
            ));
            let Some(first) = items.first() else {
                out.push_str("\nFAILED (search returned nothing)\n");
                return (1, out);
            };
            out.push_str(&format!("       first: {} — {}\n", first.title, first.id));

            let started = Instant::now();
            match provider.details(&first.id) {
                Ok(details) => out.push_str(&format!(
                    "[ok]   details: \"{}\" in {} ms\n",
                    details.item.title,
                    started.elapsed().as_millis()
                )),
                Err(err) => {
                    failures += 1;
                    out.push_str(&format!("[FAIL] details: {err}\n"));
                }
            }

            let started = Instant::now();
            match provider.resolve(&first.id, &ResolveOptions::default()) {
                Ok(sources) => {
                    out.push_str(&format!(
                        "[ok]   resolve: {} source(s) in {} ms\n",
                        sources.len(),
                        started.elapsed().as_millis()
                    ));
                    if let Some(source) = sources.first() {
                        out.push_str(&format!(
                            "       first: {} ({})\n",
                            source.url,
                            source.quality.label()
                        ));
                    }
                }
                Err(err) => {
                    failures += 1;
                    out.push_str(&format!("[FAIL] resolve: {err}\n"));
                }
            }
        }
        Err(err) => {
            failures += 1;
            out.push_str(&format!("[FAIL] search: {err}\n"));
        }
    }

    if failures == 0 {
        out.push_str("\nOK\n");
        (0, out)
    } else {
        out.push_str("\nFAILED\n");
        (1, out)
    }
}
