//! Pure, native content conversions (no subprocess): markdown <-> Teams HTML and
//! Teams HTML -> plain text. Pandoc-backed conversions live in `content::pandoc`.
//!
//! Conversions return the engine output verbatim; trimming is left to the
//! display/filter layer so nothing content-bearing (code-block indentation,
//! trailing structure) is silently dropped at the boundary.

use html2text::render::TrivialDecorator;
use pulldown_cmark::{Options, Parser};

use crate::error::ContentError;

/// Convert plain text into the HTML Teams expects: HTML-escape the reserved
/// characters and turn newlines into `<br>`, wrapped in a single `<p>` so it
/// matches the paragraph styling Teams' own composer emits.
pub fn plain_to_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 7);
    out.push_str("<p>");
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\n' => out.push_str("<br>"),
            other => out.push(other),
        }
    }
    out.push_str("</p>");
    out
}

/// Render CommonMark (plus tables, strikethrough, and task lists) into HTML.
/// pulldown-cmark wraps blocks in `<p>…</p>`, matching Teams' RichText/Html.
pub fn markdown_to_html(markdown: &str) -> String {
    let options = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let parser = Parser::new_ext(markdown, options);
    let mut html = String::with_capacity(markdown.len() * 3 / 2);
    pulldown_cmark::html::push_html(&mut html, parser);
    html
}

/// Convert Teams HTML into Markdown (via the htmd/turndown engine).
pub fn html_to_markdown(html: &str) -> Result<String, ContentError> {
    htmd::convert(html).map_err(ContentError::HtmlToMarkdown)
}

/// Convert Teams HTML into readable plain text using a real HTML parser, so
/// block boundaries (`<p>`, `<br>`, list items) become newlines and every HTML
/// entity is decoded. `width` is set high enough that message text is not
/// reflowed.
pub fn html_to_text(html: &str) -> Result<String, ContentError> {
    const WIDTH: usize = 10_000;
    html2text::config::with_decorator(TrivialDecorator::new())
        .string_from_read(html.as_bytes(), WIDTH)
        .map_err(|err| ContentError::HtmlToText(err.to_string()))
}

/// Best-effort trimmed plain-text rendering for previews (never fails; falls
/// back to the raw HTML if parsing errors).
pub fn html_to_preview(html: &str) -> String {
    html_to_text(html).map(|text| text.trim().to_owned()).unwrap_or_else(|_| html.to_owned())
}
