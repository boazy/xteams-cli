//! Message-content format handling: parse the `-I/-O/-f` format specifiers and
//! convert message bodies between plain text, HTML (the Teams wire format),
//! Markdown, and arbitrary pandoc formats.

mod convert;
pub mod mentions;
mod pandoc;

pub use convert::html_to_preview;
pub use pandoc::split_args as split_pandoc_args;

use crate::error::ContentError;
use crate::model::Message;

/// How `--content` is interpreted before it is sent to Teams (always as HTML).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentInputFormat {
    Plain,
    Html,
    Markdown,
    Pandoc(String),
}

/// How a message body read from Teams is rendered. `Keep` leaves the raw Teams
/// HTML untouched (the JSON default).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentOutputFormat {
    Keep,
    Plain,
    Html,
    Markdown,
    Pandoc(String),
}

fn pandoc_suffix(spec: &str) -> Option<&str> {
    spec.strip_prefix("pandoc:").filter(|fmt| !fmt.is_empty())
}

impl ContentInputFormat {
    fn parse(spec: &str) -> Result<Self, ContentError> {
        match spec {
            "plain" => Ok(Self::Plain),
            "html" => Ok(Self::Html),
            "markdown" | "md" => Ok(Self::Markdown),
            "keep" => Err(ContentError::KeepAsInput),
            other => pandoc_suffix(other)
                .map(|fmt| Self::Pandoc(fmt.to_owned()))
                .ok_or_else(|| ContentError::UnknownFormat(other.to_owned())),
        }
    }
}

impl ContentOutputFormat {
    fn parse(spec: &str) -> Result<Self, ContentError> {
        match spec {
            "keep" => Ok(Self::Keep),
            "plain" => Ok(Self::Plain),
            "html" => Ok(Self::Html),
            "markdown" | "md" => Ok(Self::Markdown),
            other => pandoc_suffix(other)
                .map(|fmt| Self::Pandoc(fmt.to_owned()))
                .ok_or_else(|| ContentError::UnknownFormat(other.to_owned())),
        }
    }
}

/// Resolve the input format from `-I` / `-f` (clap enforces they are mutually
/// exclusive). Defaults to Markdown.
pub fn resolve_input(
    input_format: Option<String>,
    both: Option<String>,
) -> Result<ContentInputFormat, ContentError> {
    match both.or(input_format) {
        Some(spec) => ContentInputFormat::parse(&spec),
        None => Ok(ContentInputFormat::Markdown),
    }
}

/// Resolve the output format from `-O` / `-f`. Defaults to `Keep` in JSON mode
/// (preserve the Teams wire format) and Markdown in human mode.
pub fn resolve_output(
    output_format: Option<String>,
    both: Option<String>,
    json: bool,
) -> Result<ContentOutputFormat, ContentError> {
    match both.or(output_format) {
        Some(spec) => ContentOutputFormat::parse(&spec),
        None if json => Ok(ContentOutputFormat::Keep),
        None => Ok(ContentOutputFormat::Markdown),
    }
}

/// Convert `content` (in `format`) into the HTML Teams expects on the wire.
pub fn to_teams_html(
    content: &str,
    format: &ContentInputFormat,
    pandoc_args: &[String],
) -> Result<String, ContentError> {
    match format {
        ContentInputFormat::Plain => Ok(convert::plain_to_html(content)),
        ContentInputFormat::Html => Ok(content.to_owned()),
        ContentInputFormat::Markdown => Ok(convert::markdown_to_html(content)),
        ContentInputFormat::Pandoc(fmt) => pandoc::run(fmt, "html", content, pandoc_args),
    }
}

/// Convert Teams `html` into the requested output `format`.
pub fn from_teams_html(
    html: &str,
    format: &ContentOutputFormat,
    pandoc_args: &[String],
) -> Result<String, ContentError> {
    match format {
        ContentOutputFormat::Keep | ContentOutputFormat::Html => Ok(html.to_owned()),
        ContentOutputFormat::Plain => convert::html_to_text(html),
        ContentOutputFormat::Markdown => convert::html_to_markdown(html),
        ContentOutputFormat::Pandoc(fmt) => pandoc::run("html", fmt, html, pandoc_args),
    }
}

/// Rewrite a message's `content` field in place to the requested output format.
/// `Keep` is a no-op so the raw Teams HTML survives untouched into JSON.
pub fn apply_output(
    message: &mut Message,
    format: &ContentOutputFormat,
    pandoc_args: &[String],
) -> Result<(), ContentError> {
    if *format == ContentOutputFormat::Keep {
        return Ok(());
    }
    if let Some(html) = message.content.as_deref() {
        message.content = Some(from_teams_html(html, format, pandoc_args)?);
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn strs(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn parses_input_formats() {
        assert_eq!(
            ContentInputFormat::parse("plain").unwrap(),
            ContentInputFormat::Plain
        );
        assert_eq!(
            ContentInputFormat::parse("md").unwrap(),
            ContentInputFormat::Markdown
        );
        assert_eq!(
            ContentInputFormat::parse("pandoc:rst").unwrap(),
            ContentInputFormat::Pandoc("rst".to_owned())
        );
        assert!(matches!(
            ContentInputFormat::parse("keep"),
            Err(ContentError::KeepAsInput)
        ));
        assert!(matches!(
            ContentInputFormat::parse("pandoc:"),
            Err(ContentError::UnknownFormat(_))
        ));
        assert!(matches!(
            ContentInputFormat::parse("bogus"),
            Err(ContentError::UnknownFormat(_))
        ));
    }

    #[test]
    fn resolves_format_defaults() {
        // Input defaults to markdown; -f overrides -I's absence.
        assert_eq!(
            resolve_input(None, None).unwrap(),
            ContentInputFormat::Markdown
        );
        assert_eq!(
            resolve_input(None, Some("html".to_owned())).unwrap(),
            ContentInputFormat::Html
        );
        // Output defaults: keep in JSON mode, markdown in human mode.
        assert_eq!(
            resolve_output(None, None, true).unwrap(),
            ContentOutputFormat::Keep
        );
        assert_eq!(
            resolve_output(None, None, false).unwrap(),
            ContentOutputFormat::Markdown
        );
        assert_eq!(
            resolve_output(Some("plain".to_owned()), None, true).unwrap(),
            ContentOutputFormat::Plain
        );
    }

    #[test]
    fn markdown_wraps_paragraphs_and_bold() {
        let html = convert::markdown_to_html("**Bold Text**");
        assert!(html.contains("<p>"), "expected paragraph wrapping: {html}");
        assert!(
            html.contains("<strong>Bold Text</strong>"),
            "expected bold: {html}"
        );
    }

    #[test]
    fn plain_escapes_and_wraps() {
        assert_eq!(convert::plain_to_html("a<b & c"), "<p>a&lt;b &amp; c</p>");
        assert_eq!(convert::plain_to_html("x\ny"), "<p>x<br>y</p>");
    }

    #[test]
    fn html_to_text_keeps_block_boundaries() {
        let text = convert::html_to_text("<p>one</p><p>two</p>").unwrap();
        assert!(!text.contains("onetwo"), "block boundary lost: {text:?}");
        let one = text.find("one").expect("one present");
        let two = text.find("two").expect("two present");
        assert!(
            text[one..two].contains('\n'),
            "expected newline between blocks: {text:?}"
        );
    }

    #[test]
    fn html_to_text_decodes_entities_and_quoted_attrs() {
        // Entity outside the six legacy names, plus a numeric entity.
        let text = convert::html_to_text("<p>&copy; &#8364;</p>").unwrap();
        assert!(text.contains('©'), "named entity not decoded: {text:?}");
        assert!(text.contains('€'), "numeric entity not decoded: {text:?}");
        // A quoted attribute containing &gt; must not corrupt the text.
        let link = convert::html_to_text("<a href=\"x?a=1&gt;2\">link</a>").unwrap();
        assert_eq!(link.trim(), "link");
    }

    #[test]
    fn splits_pandoc_switches_and_valued_options() {
        let (filtered, pandoc) = split_pandoc_args(strs(&[
            "xteams",
            "message",
            "new",
            "19:conv",
            "--pandoc-standalone",
            "--pandoc-css=water.css",
            "--pandoc-metadata",
            "title=Documentation",
        ]));
        assert_eq!(filtered, strs(&["xteams", "message", "new", "19:conv"]));
        assert_eq!(
            pandoc,
            strs(&[
                "--standalone",
                "--css=water.css",
                "--metadata",
                "title=Documentation"
            ])
        );
    }

    #[test]
    fn switch_before_positional_is_not_consumed() {
        // `standalone` is a switch (not a value option), so the following
        // positional stays in the clap argv.
        let (filtered, pandoc) = split_pandoc_args(strs(&[
            "xteams",
            "message",
            "list",
            "--pandoc-standalone",
            "19:conv",
        ]));
        assert_eq!(filtered, strs(&["xteams", "message", "list", "19:conv"]));
        assert_eq!(pandoc, strs(&["--standalone"]));
    }
}
