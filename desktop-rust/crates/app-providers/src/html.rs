//! Just enough HTML scanning for the built-in providers.
//!
//! A general DOM parser was not worth its ~2 MB here: these pages are machine
//! generated and the shapes we need are three (a card anchor, a heading, an
//! attribute). Everything below is a plain string scan, which also makes the
//! parsing rules obvious and testable against saved fixtures.

/// Byte offsets of every occurrence of `needle`.
pub fn find_all(hay: &str, needle: &str) -> Vec<usize> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(index) = hay[from..].find(needle) {
        let at = from + index;
        out.push(at);
        from = at + needle.len();
    }
    out
}

/// Value of the first `name="…"` attribute in `fragment`.
pub fn attr(fragment: &str, name: &str) -> Option<String> {
    all_attrs(fragment, name).into_iter().next()
}

/// Value of every `name="…"` attribute, in document order.
pub fn all_attrs(fragment: &str, name: &str) -> Vec<String> {
    let needle = format!("{name}=\"");
    find_all(fragment, &needle)
        .into_iter()
        .filter_map(|at| {
            let rest = &fragment[at + needle.len()..];
            rest.find('"').map(|end| rest[..end].to_string())
        })
        .collect()
}

/// Text of the first `<tag …>text</tag>`, tags stripped and entities decoded.
pub fn first_tag_text(html: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    for at in find_all(html, &open) {
        let rest = &html[at..];
        let Some(gt) = rest.find('>') else { continue };
        let after = &rest[gt + 1..];
        let Some(end) = after.find(&close) else {
            continue;
        };
        let text = strip_tags(&after[..end]);
        if !text.is_empty() {
            return Some(text);
        }
    }
    None
}

/// Remove tags, collapse whitespace, decode entities.
pub fn strip_tags(fragment: &str) -> String {
    let mut out = String::with_capacity(fragment.len());
    let mut inside = false;
    for ch in fragment.chars() {
        match ch {
            '<' => inside = true,
            '>' => inside = false,
            _ if !inside => out.push(ch),
            _ => {}
        }
    }
    unescape(&out.split_whitespace().collect::<Vec<_>>().join(" "))
}

pub fn unescape(text: &str) -> String {
    text.replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
}

/// Resolve a possibly relative URL against the site origin.
pub fn absolute(base: &str, url: &str) -> String {
    let url = url.trim();
    if url.starts_with("http://") || url.starts_with("https://") {
        url.to_string()
    } else if url.starts_with('/') {
        format!("{base}{url}")
    } else {
        format!("{base}/{url}")
    }
}

/// Read a string field out of the React Server Components payload these pages
/// embed, which is JSON with escaped quotes.
pub fn json_field(html: &str, name: &str) -> Option<String> {
    let escaped = format!("\\\"{name}\\\":\\\"");
    if let Some(at) = html.find(&escaped) {
        let rest = &html[at + escaped.len()..];
        if let Some(end) = rest.find("\\\"") {
            let value = rest[..end].trim();
            if !value.is_empty() {
                return Some(unescape(value));
            }
        }
    }
    let plain = format!("\"{name}\":\"");
    let at = html.find(&plain)?;
    let rest = &html[at + plain.len()..];
    let end = rest.find('"')?;
    let value = rest[..end].trim();
    (!value.is_empty()).then(|| unescape(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attributes_are_read_in_order() {
        let fragment = r#"<img src="/a.webp" data-fallback="https://x/b.jpg" alt="Title">"#;
        assert_eq!(attr(fragment, "alt").as_deref(), Some("Title"));
        assert_eq!(all_attrs(fragment, "src").len(), 1);
        assert!(attr(fragment, "missing").is_none());
    }

    #[test]
    fn tag_text_is_stripped_and_decoded() {
        let html = "<h1 class=\"x\">Avatar &amp; Friends</h1>";
        assert_eq!(
            first_tag_text(html, "h1").as_deref(),
            Some("Avatar & Friends")
        );
    }

    #[test]
    fn urls_resolve_against_the_origin() {
        assert_eq!(absolute("https://s.test", "/m/a"), "https://s.test/m/a");
        assert_eq!(
            absolute("https://s.test", "https://c.test/x"),
            "https://c.test/x"
        );
    }

    #[test]
    fn escaped_json_fields_are_found() {
        let html = r#"push([1,"{\"name\":\"Avatar\",\"overview\":\"A film.\"}"])"#;
        assert_eq!(json_field(html, "name").as_deref(), Some("Avatar"));
        assert_eq!(json_field(html, "overview").as_deref(), Some("A film."));
        assert!(json_field(html, "absent").is_none());
    }
}
