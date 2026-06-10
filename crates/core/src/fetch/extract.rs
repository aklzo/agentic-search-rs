use scraper::{Html, Selector};

/// Tags whose text content is noise for research purposes.
const SKIP_TAGS: &[&str] = &["script", "style", "noscript", "svg", "iframe", "template"];

/// Convert an HTML document into readable plain text, truncated to
/// `max_chars` on a char boundary.
pub fn html_to_text(html: &str, max_chars: usize) -> String {
    let document = Html::parse_document(html);
    let body_sel = Selector::parse("body").expect("static selector");
    let root = document
        .select(&body_sel)
        .next()
        .unwrap_or_else(|| document.root_element());
    truncate_chars(&collapse_whitespace(&visible_text(root)), max_chars)
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
}
