use dom_smoothie::Readability;
use scraper::{Html, Selector};

/// Tags whose text content is noise for research purposes.
const SKIP_TAGS: &[&str] = &["script", "style", "noscript", "svg", "iframe", "template"];

/// Below this many characters, a readability extraction is treated as a miss
/// (search-result pages, JS shells, etc.) and we fall back to the full-DOM
/// text so the LLM still gets *something*.
const MIN_READABLE_CHARS: usize = 200;

/// Convert an HTML document into readable plain text, truncated to
/// `max_chars` on a char boundary.
///
/// First tries a Readability extraction (Firefox Reader View equivalent) to
/// drop navigation/footer/boilerplate, which both improves extraction quality
/// and cuts the prompt size fed to the LLM. Falls back to a whole-document
/// text walk when no article body is found.
pub fn html_to_text(html: &str, max_chars: usize) -> String {
    let text = readable_text(html).unwrap_or_else(|| full_document_text(html));
    truncate_chars(&collapse_whitespace(&text), max_chars)
}

/// Extract just the main article body via Readability. Returns `None` when no
/// article is found or the result is too short to be a real article.
fn readable_text(html: &str) -> Option<String> {
    let mut readability = Readability::new(html, None, None).ok()?;
    let article = readability.parse().ok()?;
    let text = article.text_content.to_string();
    if text.trim().chars().count() < MIN_READABLE_CHARS {
        return None;
    }
    Some(text)
}

/// Whole-document visible text, skipping non-content tags. Used as a fallback
/// when Readability cannot isolate an article.
fn full_document_text(html: &str) -> String {
    let document = Html::parse_document(html);
    let body_sel = Selector::parse("body").expect("static selector");
    let root = document
        .select(&body_sel)
        .next()
        .unwrap_or_else(|| document.root_element());
    visible_text(root)
}

/// Depth-first text collection that skips non-visible subtrees.
fn visible_text(root: scraper::ElementRef<'_>) -> String {
    let mut out = String::new();
    let mut stack = vec![*root];
    while let Some(node) = stack.pop() {
        if let Some(text) = node.value().as_text() {
            out.push_str(text);
            out.push(' ');
            continue;
        }
        if let Some(element) = node.value().as_element() {
            if SKIP_TAGS.contains(&element.name()) {
                continue;
            }
        }
        let children: Vec<_> = node.children().collect();
        for child in children.into_iter().rev() {
            stack.push(child);
        }
    }
    out
}

fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    match text.char_indices().nth(max_chars) {
        Some((byte_index, _)) => text[..byte_index].to_string(),
        None => text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_visible_text_and_skips_scripts() {
        let html = r#"<html><head><title>T</title><style>p{color:red}</style></head>
            <body><h1>Heading</h1><script>var secret = 1;</script>
            <p>First   paragraph.</p><p>Second.</p></body></html>"#;
        let text = html_to_text(html, 1000);
        assert_eq!(text, "Heading First paragraph. Second.");
        assert!(!text.contains("secret"));
        assert!(!text.contains("color"));
    }

    #[test]
    fn truncates_to_char_boundary() {
        let html = "<body><p>日本語のテキストです</p></body>";
        let text = html_to_text(html, 3);
        assert_eq!(text, "日本語");
    }

    #[test]
    fn handles_documents_without_body() {
        let text = html_to_text("just plain text", 100);
        assert_eq!(text, "just plain text");
    }

    #[test]
    fn readability_strips_navigation_and_footer_boilerplate() {
        // A realistic article page: the <article> body is long enough to be
        // detected, surrounded by nav/footer noise that must be dropped.
        let body = "Tokio is an asynchronous runtime for the Rust programming language. \
            It provides the building blocks needed for writing networking applications. \
            The runtime includes a multi-threaded, work-stealing scheduler and an \
            event-driven, non-blocking I/O reactor. Tasks are lightweight green threads \
            that begin running immediately when spawned onto the runtime scheduler.";
        let html = format!(
            r#"<html><body>
                <nav><a href="/">Home</a><a href="/login">Sign in to your account</a></nav>
                <header>Cookie consent banner: accept all cookies to continue browsing</header>
                <article><h1>The Tokio Runtime</h1><p>{body}</p><p>{body}</p></article>
                <footer>Copyright 2026 Example Corp. All rights reserved. Privacy policy.</footer>
            </body></html>"#
        );
        let text = html_to_text(&html, 5000);
        assert!(
            text.contains("asynchronous runtime"),
            "keeps the article body"
        );
        assert!(!text.contains("Sign in"), "drops nav");
        assert!(!text.contains("Copyright 2026"), "drops footer");
        assert!(!text.contains("Cookie consent"), "drops banner");
    }
}
