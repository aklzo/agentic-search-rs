use async_trait::async_trait;
use scraper::{Html, Selector};
use url::Url;

use super::{SearchHit, SearchProvider};
use crate::error::{AgentError, Result};

const ENDPOINT: &str = "https://html.duckduckgo.com/html/";

/// Keyless search via DuckDuckGo's HTML endpoint. Default provider so the
/// tool works out of the box; prefer SearXNG for heavier use.
pub struct DuckDuckGo {
    http: reqwest::Client,
}

impl DuckDuckGo {
    pub fn new(http: reqwest::Client) -> Self {
        Self { http }
    }
}

#[async_trait]
impl SearchProvider for DuckDuckGo {
    async fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
        let response = self
            .http
            .get(ENDPOINT)
            .query(&[("q", query)])
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(AgentError::Search(format!(
                "duckduckgo returned HTTP {}",
                response.status()
            )));
        }
        let html = response.text().await?;
        let mut hits = parse_results(&html);
        hits.truncate(limit);
        Ok(hits)
    }
}

/// Parse the result list out of the DuckDuckGo HTML page.
fn parse_results(html: &str) -> Vec<SearchHit> {
    let document = Html::parse_document(html);
    let result_sel = Selector::parse("div.result").expect("static selector");
    let link_sel = Selector::parse("a.result__a").expect("static selector");
    let snippet_sel = Selector::parse(".result__snippet").expect("static selector");

    document
        .select(&result_sel)
        .filter_map(|result| {
            let link = result.select(&link_sel).next()?;
            let href = link.value().attr("href")?;
            let url = resolve_redirect(href)?;
            let title = collect_text(link);
            let snippet = result
                .select(&snippet_sel)
                .next()
                .map(collect_text)
                .unwrap_or_default();
            Some(SearchHit {
                title,
                url,
                snippet,
            })
        })
        .collect()
}

/// DuckDuckGo wraps result URLs in a `/l/?uddg=<encoded>` redirect.
fn resolve_redirect(href: &str) -> Option<String> {
    let absolute = if href.starts_with("//") {
        format!("https:{href}")
    } else {
        href.to_string()
    };
    let url = Url::parse(&absolute).ok()?;
    if url.path() == "/l/" {
        let (_, target) = url.query_pairs().find(|(key, _)| key == "uddg")?;
        return Some(target.into_owned());
    }
    Some(absolute)
}

fn collect_text(element: scraper::ElementRef<'_>) -> String {
    element.text().collect::<String>().trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"
    <html><body>
      <div class="result">
        <a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2Fpage&rut=abc">Example Title</a>
        <a class="result__snippet">An example snippet.</a>
      </div>
      <div class="result">
        <a class="result__a" href="https://direct.example.org/doc">Direct Link</a>
      </div>
      <div class="result"><span>no link here</span></div>
    </body></html>"#;

    #[test]
    fn parses_redirect_wrapped_and_direct_results() {
        let hits = parse_results(FIXTURE);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].url, "https://example.com/page");
        assert_eq!(hits[0].title, "Example Title");
        assert_eq!(hits[0].snippet, "An example snippet.");
        assert_eq!(hits[1].url, "https://direct.example.org/doc");
        assert!(hits[1].snippet.is_empty());
    }

    #[test]
    fn empty_page_yields_no_hits() {
        assert!(parse_results("<html><body></body></html>").is_empty());
    }
}
