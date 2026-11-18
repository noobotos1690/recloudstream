//! CTGMovies: a port of the `CTGMovies` CloudStream plugin.
//!
//! The original is Kotlin running inside CloudStream on Android. What it does is
//! plain HTTP and HTML, so it ports directly:
//!
//! * listings (`/movies`, `/tv`, `/anime`) and `/search?q=` share one card shape
//! * a detail page carries the title, artwork and *direct file URLs* (`.mp4`,
//!   `.mkv`) plus subtitle files, which is what the player needs
//!
//! Nothing is scraped from the plugin binary; the behaviour comes from its
//! published source and from the live markup, which the fixtures pin down.

use crate::html;
use app_core::{
    HomeRow, HttpClient, HttpRequest, MediaDetails, MediaItem, MediaKind, Provider,
    ProviderCapabilities, ProviderError, ProviderManifest, Quality, ResolveOptions, RowKind,
    StreamSource, SubtitleTrack,
};
use std::sync::Arc;

const BASE: &str = "https://ctgmovies.com";
const ID: &str = crate::CTGMOVIES_ID;
const NAME: &str = "CTGMovies";
const VERSION: &str = "1.0.0";
/// Listing and detail pages are image-heavy: about 350 KB each in practice.
const MAX_PAGE_BYTES: usize = 8 * 1024 * 1024;
/// Long enough for a slow streaming page, short enough to give up on a dead one.
const PAGE_TIMEOUT_SECS: u64 = 60;
const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) \
     Chrome/124.0 Safari/537.36";

pub struct CtgMovies {
    http: Arc<dyn HttpClient>,
}

impl CtgMovies {
    pub fn new(http: Arc<dyn HttpClient>) -> Self {
        Self { http }
    }

    fn get(&self, url: &str) -> Result<String, ProviderError> {
        let mut request = HttpRequest::get(url);
        request.max_bytes = MAX_PAGE_BYTES;
        // These pages are streamed and can stall for tens of seconds under load.
        // The timeout is per request on purpose: it applies without changing the
        // transport's default for everything else.
        request.timeout = Some(std::time::Duration::from_secs(PAGE_TIMEOUT_SECS));
        request
            .headers
            .push(("User-Agent".into(), USER_AGENT.into()));
        request
            .headers
            .push(("Accept".into(), "text/html,application/xhtml+xml".into()));
        let response = self.http.execute(request)?;
        if !response.is_success() {
            return Err(ProviderError::Network(format!(
                "{url} returned HTTP {}",
                response.status
            )));
        }
        Ok(response.text())
    }
}

impl Provider for CtgMovies {
    fn manifest(&self) -> ProviderManifest {
        ProviderManifest {
            id: ID.to_string(),
            name: NAME.to_string(),
            version: VERSION.to_string(),
            description: "Films, series and anime, with direct file links for playback."
                .to_string(),
            language: Some("en".to_string()),
            icon_url: None,
            capabilities: ProviderCapabilities {
                home: true,
                search: true,
                details: true,
                resolve: true,
                live: false,
            },
        }
    }

    fn home(&self) -> Result<Vec<HomeRow>, ProviderError> {
        let mut rows = Vec::new();
        let mut failures = Vec::new();
        for (path, title) in [
            ("/movies", "Latest movies"),
            ("/tv", "Series"),
            ("/anime", "Anime"),
        ] {
            match self.get(&format!("{BASE}{path}")) {
                Ok(page) => {
                    let items = parse_cards(&page);
                    if !items.is_empty() {
                        rows.push(HomeRow {
                            title: title.to_string(),
                            kind: RowKind::Latest,
                            items,
                        });
                    }
                }
                Err(err) => failures.push(format!("{path}: {err}")),
            }
        }
        if rows.is_empty() {
            return Err(ProviderError::Network(if failures.is_empty() {
                "the site returned no listings".to_string()
            } else {
                failures.join("; ")
            }));
        }
        Ok(rows)
    }

    fn search(&self, query: &str) -> Result<Vec<MediaItem>, ProviderError> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return Ok(Vec::new());
        }
        let page = self.get(&format!("{BASE}/search?q={}", encode(trimmed)))?;
        Ok(parse_cards(&page))
    }

    fn details(&self, id: &str) -> Result<MediaDetails, ProviderError> {
        let url = html::absolute(BASE, id);
        let page = self.get(&url)?;

        let title = html::first_tag_text(&page, "h1")
            .map(|text| html::unescape(&text))
            .unwrap_or_else(|| "Untitled".to_string());

        let mut poster = None;
        let mut backdrop = None;
        for candidate in html::all_attrs(&page, "data-fallback") {
            let candidate = html::absolute(BASE, &candidate);
            if candidate.contains("/w1280/") || candidate.contains("backdrop") {
                backdrop.get_or_insert(candidate);
            } else if candidate.contains("/w500/") {
                poster.get_or_insert(candidate);
            }
        }

        let description = html::json_field(&page, "overview");
        let genres: Vec<String> = html::json_field(&page, "genres")
            .map(|raw| {
                raw.split(',')
                    .map(|genre| genre.trim().to_string())
                    .filter(|genre| !genre.is_empty())
                    .collect()
            })
            .unwrap_or_default();

        let mut item = MediaItem::new(ID, url.clone(), title);
        item.kind = kind_for(&url);
        item.poster = poster.or_else(|| backdrop.clone());
        item.year = find_year(&page);
        Ok(MediaDetails {
            item,
            description,
            backdrop,
            genres,
            episodes: Vec::new(),
        })
    }

    fn resolve(
        &self,
        id: &str,
        _options: &ResolveOptions,
    ) -> Result<Vec<StreamSource>, ProviderError> {
        let url = html::absolute(BASE, id);
        let page = self.get(&url)?;

        let links = media_links(&page);
        let subtitles: Vec<SubtitleTrack> = links
            .iter()
            .filter(|link| is_subtitle(link))
            .map(|link| SubtitleTrack {
                url: link.clone(),
                language: language_of(link).to_string(),
                label: None,
                default: false,
            })
            .collect();

        let mut sources: Vec<StreamSource> = Vec::new();
        for link in links.iter().filter(|link| !is_subtitle(link)) {
            if sources.iter().any(|source| &source.url == link) {
                continue;
            }
            let mut source = StreamSource::new(link.clone());
            source.quality = quality_of(link);
            source.subtitles = subtitles.clone();
            source.label = Some(file_name(link));
            sources.push(source);
        }

        if sources.is_empty() {
            return Err(ProviderError::NotFound(
                "this page has no direct video link; open it in a browser to check".to_string(),
            ));
        }
        sources.sort_by_key(|source| std::cmp::Reverse(source.quality.rank()));
        Ok(sources)
    }
}

// ---------------------------------------------------------------------------
// parsing
// ---------------------------------------------------------------------------

/// Every card on a listing or search page.
fn parse_cards(page: &str) -> Vec<MediaItem> {
    let mut items = Vec::new();
    for at in html::find_all(page, "<a class=\"block\"") {
        let rest = &page[at..];
        let Some(end) = rest.find("</a>") else {
            continue;
        };
        let block = &rest[..end];
        let Some(href) = html::attr(block, "href") else {
            continue;
        };
        let url = html::absolute(BASE, &href);
        // The image alt is the most reliable title: the visible heading drops
        // long names with `truncate`.
        let title = html::attr(block, "alt")
            .map(|alt| html::unescape(&alt))
            .unwrap_or_default();
        if title.trim().is_empty() {
            continue;
        }
        let poster = html::attr(block, "data-fallback")
            .or_else(|| html::attr(block, "src"))
            .map(|src| html::absolute(BASE, &src))
            .filter(|src| src.starts_with("http"));

        let mut item = MediaItem::new(ID, url.clone(), title);
        item.kind = kind_for(&url);
        item.poster = poster;
        item.year = find_year(block);
        items.push(item);
    }
    items
}

fn kind_for(url: &str) -> MediaKind {
    if url.contains("/tv/") {
        MediaKind::Series
    } else if url.contains("/movies/") {
        MediaKind::Movie
    } else {
        MediaKind::Other
    }
}

/// A four-digit year rendered in a mono-font caption.
fn find_year(fragment: &str) -> Option<u16> {
    for at in html::find_all(fragment, "font-mono") {
        let rest = &fragment[at..];
        let Some(gt) = rest.find('>') else { continue };
        let tail = &rest[gt + 1..];
        let end = tail.find('<').unwrap_or(tail.len().min(8));
        let text = tail[..end].trim();
        if text.len() == 4 && text.chars().all(|c| c.is_ascii_digit()) {
            return text.parse().ok();
        }
    }
    None
}

/// Direct media and subtitle URLs anywhere in the page.
fn media_links(page: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for at in html::find_all(page, "http") {
        let rest = &page[at..];
        let end = rest
            .find(|c: char| {
                matches!(
                    c,
                    '"' | '\'' | '\\' | '<' | '>' | ' ' | '\n' | '\r' | '\t' | ')' | '(' | ','
                )
            })
            .unwrap_or(rest.len());
        let candidate = &rest[..end];
        let candidate = candidate.replace("&amp;", "&");
        if !is_media(&candidate) || out.contains(&candidate) {
            continue;
        }
        out.push(candidate);
    }
    out
}

fn path_of(url: &str) -> &str {
    url.split(['?', '#']).next().unwrap_or(url)
}

fn extension(url: &str) -> &str {
    let path = path_of(url);
    path.rsplit_once('.').map(|(_, ext)| ext).unwrap_or("")
}

fn is_subtitle(url: &str) -> bool {
    matches!(extension(url).to_ascii_lowercase().as_str(), "srt" | "vtt")
}

fn is_media(url: &str) -> bool {
    matches!(
        extension(url).to_ascii_lowercase().as_str(),
        "mkv" | "mp4" | "m3u8" | "webm" | "srt" | "vtt"
    )
}

fn quality_of(url: &str) -> Quality {
    let name = path_of(url).to_ascii_lowercase();
    if name.contains("2160") || name.contains("4k") {
        Quality::Uhd2160
    } else if name.contains("1080") {
        Quality::Fhd1080
    } else if name.contains("720") {
        Quality::Hd720
    } else if name.contains("480") {
        Quality::Sd480
    } else {
        Quality::Unknown
    }
}

fn language_of(url: &str) -> &'static str {
    let name = path_of(url).to_ascii_lowercase();
    if name.contains("hindi") || name.contains(".hin.") {
        "Hindi"
    } else if name.contains("bangla") || name.contains("bengali") {
        "Bengali"
    } else {
        "English"
    }
}

fn file_name(url: &str) -> String {
    path_of(url)
        .rsplit('/')
        .next()
        .map(urldecode)
        .unwrap_or_default()
}

fn urldecode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = &input[index + 1..index + 3];
            if let Ok(value) = u8::from_str_radix(hex, 16) {
                out.push(value);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Percent-encode a query string value.
fn encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use app_core::{HttpResponse, HttpVerb};
    use std::sync::Mutex;

    const SEARCH: &str = include_str!("../tests/fixtures/search.html");
    const DETAIL: &str = include_str!("../tests/fixtures/detail.html");

    struct StubHttp {
        body: String,
        seen: Mutex<Vec<String>>,
    }

    impl HttpClient for StubHttp {
        fn execute(&self, request: HttpRequest) -> Result<HttpResponse, ProviderError> {
            assert_eq!(request.verb, HttpVerb::Get);
            self.seen.lock().unwrap().push(request.url);
            Ok(HttpResponse {
                status: 200,
                headers: vec![],
                body: self.body.clone().into_bytes(),
            })
        }
    }

    fn provider(body: &str) -> (CtgMovies, Arc<StubHttp>) {
        let http = Arc::new(StubHttp {
            body: body.to_string(),
            seen: Mutex::new(Vec::new()),
        });
        (CtgMovies::new(http.clone()), http)
    }

    #[test]
    fn cards_become_items_with_kind_and_year() {
        let items = parse_cards(SEARCH);
        assert_eq!(items.len(), 2, "fixture has two cards");
        assert_eq!(items[0].title, "Avatar");
        assert_eq!(items[0].kind, MediaKind::Movie);
        assert_eq!(items[0].id, "https://ctgmovies.com/movies/avatar");
        assert_eq!(items[0].year, Some(2009));
        assert!(items[0].poster.as_deref().unwrap().contains("tmdb.org"));
        assert_eq!(items[1].kind, MediaKind::Series);
    }

    #[test]
    fn search_hits_the_search_endpoint() {
        let (provider, http) = provider(SEARCH);
        let items = provider.search("avatar").expect("search parses");
        assert_eq!(items.len(), 2);
        assert_eq!(
            http.seen.lock().unwrap()[0],
            "https://ctgmovies.com/search?q=avatar"
        );
    }

    #[test]
    fn empty_query_does_no_request() {
        let (provider, http) = provider(SEARCH);
        assert!(provider.search("  ").unwrap().is_empty());
        assert!(http.seen.lock().unwrap().is_empty());
    }

    #[test]
    fn details_expose_title_artwork_and_description() {
        let (provider, _) = provider(DETAIL);
        let details = provider
            .details("https://ctgmovies.com/movies/avatar")
            .expect("details parse");
        assert_eq!(details.item.title, "Avatar");
        assert_eq!(details.item.kind, MediaKind::Movie);
        assert!(details.item.poster.as_deref().unwrap().contains("/w500/"));
        assert!(details.backdrop.as_deref().unwrap().contains("/w1280/"));
        assert_eq!(
            details.description.as_deref(),
            Some("A paraplegic marine is sent to Pandora.")
        );
        assert!(details.genres.contains(&"Action".to_string()));
    }

    #[test]
    fn resolve_returns_direct_links_with_subtitles() {
        let (provider, _) = provider(DETAIL);
        let sources = provider
            .resolve(
                "https://ctgmovies.com/movies/avatar",
                &ResolveOptions::default(),
            )
            .expect("sources parse");
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].quality, Quality::Fhd1080);
        assert!(sources[0].url.ends_with(".mp4"));
        assert_eq!(sources[0].subtitles.len(), 1);
        assert!(sources[0].subtitles[0].url.ends_with(".srt"));
        assert!(sources[0].label.as_deref().unwrap().contains("Avatar"));
    }

    #[test]
    fn quality_and_language_come_from_the_file_name() {
        assert_eq!(quality_of("https://x/a.2160p.mkv"), Quality::Uhd2160);
        assert_eq!(quality_of("https://x/a.720p.mp4"), Quality::Hd720);
        assert_eq!(language_of("https://x/a.hindi.srt"), "Hindi");
        assert_eq!(language_of("https://x/a.mp4"), "English");
    }

    #[test]
    fn query_values_are_encoded() {
        assert_eq!(encode("spider man"), "spider%20man");
        assert_eq!(encode("a&b"), "a%26b");
    }
}
