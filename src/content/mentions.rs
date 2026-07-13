//! `@{…}` mention tokens. After the body is converted to wire HTML, text nodes
//! may carry mention tokens: `@{query}` (person, via people search), `@{#name}`
//! (channel, via the CSA roster), or `@{<mri>|<Display Name>}` (explicit, no
//! lookup). `parse` splits the HTML into literal pieces and specs; the command
//! layer resolves each spec to an `(mri, display name)` pair; `assemble`
//! re-emits the HTML with the Skype mention `<span>`s and builds the
//! `properties.mentions` entries Teams needs for a mention to actually notify
//! (a bare `<at>`/name in HTML does nothing).
//!
//! Tokens inside `<code>`/`<pre>` stay literal; `@@{` escapes a literal `@{`.

mod pick;

pub use pick::{pick_channel, pick_person};

use serde::Serialize;

use crate::error::ContentError;

pub const MENTION_SCHEMA: &str = "http://schema.skype.com/Mention";

/// One entry of the `properties.mentions` array (the wire carries the array
/// JSON-encoded *as a string*). Emitted `mentionType`s: `person` (`8:…` MRIs)
/// and `channel` (`19:…` thread MRIs) — the shapes verified live.
#[derive(Debug, Clone, Serialize)]
pub struct Mention {
    #[serde(rename = "@type")]
    pub schema: &'static str,
    pub itemid: usize,
    pub mri: String,
    #[serde(rename = "mentionType")]
    pub mention_type: &'static str,
    #[serde(rename = "displayName")]
    pub display_name: String,
}

/// A parsed `@{…}` token, before resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MentionSpec {
    /// `@{query}` — resolve to a person via people search.
    Query(String),
    /// `@{#name}` / `@{#team/name}` — resolve to a channel via the CSA roster.
    ChannelQuery(String),
    /// `@{<mri>|<name>}` — caller supplied the MRI; no lookup.
    Explicit { mri: String, name: String },
}

enum Piece {
    /// Literal HTML (with `@@{` already unescaped where applicable).
    Text(String),
    /// The mention with this index (into `specs` / the resolved slice).
    Mention(usize),
}

/// The wire HTML split around its mention tokens.
pub struct MentionDoc {
    pieces: Vec<Piece>,
    specs: Vec<MentionSpec>,
}

impl MentionDoc {
    pub fn specs(&self) -> &[MentionSpec] {
        &self.specs
    }

    /// Re-emit the HTML with a mention span per token; `resolved[i]` is the
    /// `(mri, display name)` for `specs()[i]`. Returns the final HTML and the
    /// `properties.mentions` entries (itemids match the spans).
    pub fn assemble(&self, resolved: &[(String, String)]) -> (String, Vec<Mention>) {
        let mentions: Vec<Mention> = resolved
            .iter()
            .enumerate()
            .map(|(itemid, (mri, name))| Mention {
                schema: MENTION_SCHEMA,
                itemid,
                mri: mri.clone(),
                mention_type: mention_type_for(mri),
                display_name: name.clone(),
            })
            .collect();
        let mut html = String::new();
        for piece in &self.pieces {
            match piece {
                Piece::Text(text) => html.push_str(text),
                Piece::Mention(i) => {
                    if let Some(m) = mentions.get(*i) {
                        html.push_str(&format!(
                            "<span itemtype=\"{MENTION_SCHEMA}\" itemscope=\"\" itemid=\"{}\">{}</span>",
                            m.itemid,
                            escape_html(&m.display_name)
                        ));
                    }
                }
            }
        }
        (html, mentions)
    }
}

/// Split wire HTML into literal pieces and mention specs. Tags are copied
/// verbatim (tokens never match inside attributes); text inside `<code>` or
/// `<pre>` is literal.
pub fn parse(html: &str) -> Result<MentionDoc, ContentError> {
    let mut doc = MentionDoc {
        pieces: Vec::new(),
        specs: Vec::new(),
    };
    let mut literal = String::new();
    let mut code_depth = 0usize;
    let mut rest = html;
    while !rest.is_empty() {
        if rest.starts_with('<') {
            let len = tag_end(rest);
            let (tag, after) = rest.split_at(len);
            adjust_code_depth(tag, &mut code_depth);
            literal.push_str(tag);
            rest = after;
        } else {
            let len = rest.find('<').unwrap_or(rest.len());
            let (text, after) = rest.split_at(len);
            if code_depth > 0 {
                literal.push_str(text);
            } else {
                scan_text(text, &mut literal, &mut doc)?;
            }
            rest = after;
        }
    }
    if !literal.is_empty() {
        doc.pieces.push(Piece::Text(literal));
    }
    Ok(doc)
}

/// Length of the tag starting at `rest[0] == '<'`, honoring quoted attribute
/// values (a `>` inside `"…"` / `'…'` does not close the tag). An unterminated
/// tag runs to the end of the input.
fn tag_end(rest: &str) -> usize {
    let mut quote: Option<u8> = None;
    for (i, b) in rest.bytes().enumerate().skip(1) {
        match quote {
            Some(q) if b == q => quote = None,
            Some(_) => {}
            None => match b {
                b'"' | b'\'' => quote = Some(b),
                b'>' => return i + 1,
                _ => {}
            },
        }
    }
    rest.len()
}

/// Track nesting of literal-content elements (`<code>`, `<pre>`).
fn adjust_code_depth(tag: &str, depth: &mut usize) {
    let name = tag
        .trim_start_matches('<')
        .trim_start_matches('/')
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();
    if name == "code" || name == "pre" {
        if tag.starts_with("</") {
            *depth = depth.saturating_sub(1);
        } else if !tag.ends_with("/>") {
            *depth += 1;
        }
    }
}

/// Scan one text node for `@{…}` tokens, flushing literal runs into pieces.
fn scan_text(
    mut text: &str,
    literal: &mut String,
    doc: &mut MentionDoc,
) -> Result<(), ContentError> {
    while let Some(i) = text.find("@{") {
        if i > 0 && text.as_bytes()[i - 1] == b'@' {
            // `@@{` escapes a literal `@{`.
            literal.push_str(&text[..i - 1]);
            literal.push_str("@{");
            text = &text[i + 2..];
            continue;
        }
        literal.push_str(&text[..i]);
        let body = &text[i + 2..];
        let end = body.find('}').ok_or(ContentError::UnterminatedMention)?;
        let spec = parse_spec(&body[..end])?;
        doc.pieces.push(Piece::Text(std::mem::take(literal)));
        doc.pieces.push(Piece::Mention(doc.specs.len()));
        doc.specs.push(spec);
        text = &body[end + 1..];
    }
    literal.push_str(text);
    Ok(())
}

/// Parse the inside of `@{…}`. With a `|`, the first segment must be a person
/// (`8:…`) or channel (`19:…`) MRI; `#…` is a channel query; anything else is
/// a person search query.
fn parse_spec(raw: &str) -> Result<MentionSpec, ContentError> {
    let raw = unescape_html(raw);
    match raw.split_once('|') {
        None => {
            let query = raw.trim();
            if let Some(channel) = query.strip_prefix('#') {
                let channel = channel.trim();
                if channel.is_empty() {
                    return Err(ContentError::EmptyMention);
                }
                return Ok(MentionSpec::ChannelQuery(channel.to_owned()));
            }
            if query.is_empty() {
                return Err(ContentError::EmptyMention);
            }
            Ok(MentionSpec::Query(query.to_owned()))
        }
        Some((mri, name)) => {
            let (mri, name) = (mri.trim(), name.trim());
            if !is_person_mri(mri) && !is_channel_mri(mri) {
                return Err(ContentError::BadMentionMri(mri.to_owned()));
            }
            if name.is_empty() {
                return Err(ContentError::EmptyMention);
            }
            Ok(MentionSpec::Explicit {
                mri: mri.to_owned(),
                name: name.to_owned(),
            })
        }
    }
}

/// Person MRIs start with kind `8` (`8:orgid:<oid>`, guests, federated users).
fn is_person_mri(s: &str) -> bool {
    s.strip_prefix("8:").is_some_and(|body| !body.is_empty())
}

/// Channel MRIs are the channel's own thread id, `19:…@thread.tacv2` — the
/// only channel shape verified live (group-chat threads like `19:…@thread.v2`
/// are not valid channel mentions).
fn is_channel_mri(s: &str) -> bool {
    s.strip_prefix("19:")
        .is_some_and(|body| body.ends_with("@thread.tacv2") && body.len() > 13)
}

/// The `mentionType` for an MRI: `19:` thread MRIs mention a channel,
/// everything else a person (only those two kinds pass `parse_spec`).
fn mention_type_for(mri: &str) -> &'static str {
    if is_channel_mri(mri) {
        "channel"
    } else {
        "person"
    }
}

fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            other => out.push(other),
        }
    }
    out
}

/// Undo the entity escaping a content conversion may have applied to token text.
fn unescape_html(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn explicit(mri: &str, name: &str) -> (String, String) {
        (mri.to_owned(), name.to_owned())
    }

    #[test]
    fn no_tokens_round_trips_verbatim() {
        let html = r#"<p>plain <b>text</b> a@b.com</p>"#;
        let doc = parse(html).unwrap();
        assert!(doc.specs().is_empty());
        assert_eq!(doc.assemble(&[]).0, html);
    }

    #[test]
    fn token_becomes_span_and_metadata_with_aligned_itemids() {
        let doc = parse("<p>hi @{8:orgid:abc|Ada} and @{Bob}</p>").unwrap();
        assert_eq!(doc.specs().len(), 2);
        assert_eq!(
            doc.specs()[0],
            MentionSpec::Explicit {
                mri: "8:orgid:abc".into(),
                name: "Ada".into()
            }
        );
        assert_eq!(doc.specs()[1], MentionSpec::Query("Bob".into()));
        let (html, mentions) = doc.assemble(&[
            explicit("8:orgid:abc", "Ada"),
            explicit("8:orgid:def", "Bob X"),
        ]);
        assert_eq!(
            html,
            format!(
                "<p>hi <span itemtype=\"{MENTION_SCHEMA}\" itemscope=\"\" itemid=\"0\">Ada</span> \
                 and <span itemtype=\"{MENTION_SCHEMA}\" itemscope=\"\" itemid=\"1\">Bob X</span></p>"
            )
        );
        assert_eq!(mentions.len(), 2);
        assert_eq!(
            (mentions[0].itemid, mentions[0].mri.as_str()),
            (0, "8:orgid:abc")
        );
        assert_eq!(
            (mentions[1].itemid, mentions[1].display_name.as_str()),
            (1, "Bob X")
        );
    }

    #[test]
    fn escaped_and_code_tokens_stay_literal() {
        let doc = parse("<p>@@{nope} <code>x @{nope} y</code><pre>@{also}</pre></p>").unwrap();
        assert!(doc.specs().is_empty());
        assert_eq!(
            doc.assemble(&[]).0,
            "<p>@{nope} <code>x @{nope} y</code><pre>@{also}</pre></p>"
        );
    }

    #[test]
    fn quoted_gt_in_attribute_does_not_split_the_tag() {
        let html = r#"<p><a title="x > @{Alice}" href="u">link</a> @{8:a|A}</p>"#;
        let doc = parse(html).unwrap();
        assert_eq!(doc.specs().len(), 1, "attribute token must not be parsed");
        let (out, _) = doc.assemble(&[explicit("8:a", "A")]);
        assert!(
            out.starts_with(r#"<p><a title="x > @{Alice}" href="u">link</a> "#),
            "{out}"
        );
    }

    #[test]
    fn display_name_and_entities_are_escaped_correctly() {
        // The conversion escaped `&` in the token; the emitted span re-escapes.
        let doc = parse("<p>@{8:orgid:abc|Smith &amp; Jones}</p>").unwrap();
        assert_eq!(
            doc.specs()[0],
            MentionSpec::Explicit {
                mri: "8:orgid:abc".into(),
                name: "Smith & Jones".into()
            }
        );
        let (html, _) = doc.assemble(&[explicit("8:orgid:abc", "Smith & Jones")]);
        assert!(html.contains(">Smith &amp; Jones</span>"), "{html}");
    }

    #[test]
    fn channel_tokens_parse_and_emit_channel_metadata() {
        let doc =
            parse("<p>see @{#Proto Mapping} and @{19:c1@thread.tacv2|Proto Mapping}</p>").unwrap();
        assert_eq!(
            doc.specs()[0],
            MentionSpec::ChannelQuery("Proto Mapping".into())
        );
        assert_eq!(
            doc.specs()[1],
            MentionSpec::Explicit {
                mri: "19:c1@thread.tacv2".into(),
                name: "Proto Mapping".into()
            }
        );
        let (html, mentions) = doc.assemble(&[
            explicit("19:c1@thread.tacv2", "Proto Mapping"),
            explicit("19:c1@thread.tacv2", "Proto Mapping"),
        ]);
        assert!(html.contains("itemid=\"0\">Proto Mapping</span>"), "{html}");
        assert_eq!(mentions[0].mention_type, "channel");
        assert_eq!(mentions[1].mention_type, "channel");
    }

    #[test]
    fn person_mris_emit_person_metadata() {
        let doc = parse("<p>@{8:orgid:abc|Ada}</p>").unwrap();
        let (_, mentions) = doc.assemble(&[explicit("8:orgid:abc", "Ada")]);
        assert_eq!(mentions[0].mention_type, "person");
    }

    #[test]
    fn bad_tokens_error() {
        assert!(matches!(
            parse("<p>@{x"),
            Err(ContentError::UnterminatedMention)
        ));
        assert!(matches!(
            parse("<p>@{}</p>"),
            Err(ContentError::EmptyMention)
        ));
        assert!(matches!(
            parse("<p>@{#}</p>"),
            Err(ContentError::EmptyMention)
        ));
        assert!(matches!(
            parse("<p>@{8:orgid:abc|}</p>"),
            Err(ContentError::EmptyMention)
        ));
        // Group-chat threads and bots are not mentionable shapes.
        assert!(matches!(
            parse("<p>@{19:x@thread.v2|Chat}</p>"),
            Err(ContentError::BadMentionMri(_))
        ));
        assert!(matches!(
            parse("<p>@{28:appid|Bot}</p>"),
            Err(ContentError::BadMentionMri(_))
        ));
        assert!(matches!(
            parse("<p>@{not-an-mri|Name}</p>"),
            Err(ContentError::BadMentionMri(_))
        ));
    }
}
