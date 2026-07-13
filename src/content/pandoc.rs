//! Delegated conversions via the external `pandoc` binary, plus the argv
//! preprocessing that turns `--pandoc-<option>` flags into pandoc arguments.

use std::io::Write;
use std::process::{Command, Stdio};

use crate::error::ContentError;

/// The `--pandoc-` prefix xteams strips to build pandoc arguments.
const PREFIX: &str = "--pandoc-";

/// Long pandoc options that take a value and are therefore allowed in the
/// space-separated form (`--pandoc-metadata title=Doc`). Any option not listed
/// here is treated as a switch unless written with `=` (`--pandoc-css=x.css`),
/// which keeps a value-less pandoc flag from swallowing a following positional.
const VALUE_OPTIONS: &[&str] = &[
    "metadata",
    "metadata-file",
    "variable",
    "css",
    "template",
    "include-in-header",
    "include-before-body",
    "include-after-body",
    "highlight-style",
    "syntax-definition",
    "reference-doc",
    "toc-depth",
    "wrap",
    "columns",
    "tab-stop",
    "top-level-division",
    "number-offset",
    "shift-heading-level-by",
    "id-prefix",
    "title-prefix",
    "slide-level",
    "default-image-extension",
    "bibliography",
    "csl",
    "citation-abbreviations",
    "filter",
    "lua-filter",
    "pdf-engine",
    "pdf-engine-opt",
    "resource-path",
    "request-header",
    "abbreviations",
    "indented-code-classes",
    "data-dir",
    "extract-media",
    "eol",
    "reference-location",
    "markdown-headings",
    "ipynb-output",
];

/// Options xteams must control itself; forwarding them could redirect output or
/// change the conversion direction and hand non-HTML to the Teams API.
const RESERVED: &[&str] = &["from", "to", "output", "f", "t", "o"];

/// Split raw process arguments into (arguments for clap, pandoc passthrough
/// arguments). Each `--pandoc-<body>` becomes `--<body>`; for a value-taking
/// long option written without `=`, the following non-flag token is consumed as
/// its value. Pure so it can be unit-tested without a process.
pub fn split_args(args: Vec<String>) -> (Vec<String>, Vec<String>) {
    let mut filtered = Vec::with_capacity(args.len());
    let mut pandoc = Vec::new();
    let mut iter = args.into_iter().peekable();
    while let Some(arg) = iter.next() {
        let Some(body) = arg.strip_prefix(PREFIX).filter(|b| !b.is_empty()) else {
            filtered.push(arg);
            continue;
        };
        let name = body.split('=').next().unwrap_or(body);
        let has_inline_value = body.contains('=');
        pandoc.push(format!("--{body}"));
        if !has_inline_value
            && VALUE_OPTIONS.contains(&name)
            && iter.peek().is_some_and(|next| !next.starts_with('-'))
            && let Some(value) = iter.next()
        {
            pandoc.push(value);
        }
    }
    (filtered, pandoc)
}

/// Reject any forwarded pandoc argument that would override the conversion
/// direction or output target xteams controls.
fn check_reserved(extra: &[String]) -> Result<(), ContentError> {
    for arg in extra {
        let Some(body) = arg.strip_prefix("--").or_else(|| arg.strip_prefix('-')) else {
            continue;
        };
        let name = body.split('=').next().unwrap_or(body);
        if RESERVED.contains(&name) {
            return Err(ContentError::ReservedPandocOption(name.to_owned()));
        }
    }
    Ok(())
}

/// Run `pandoc --from <from> --to <to> <extra…>`, feeding `input` on stdin and
/// returning stdout. The invariant `--from`/`--to` are placed first and reserved
/// options are rejected, so `extra` can never change the conversion direction.
pub fn run(from: &str, to: &str, input: &str, extra: &[String]) -> Result<String, ContentError> {
    check_reserved(extra)?;
    let mut child = Command::new("pandoc")
        .arg("--from")
        .arg(from)
        .arg("--to")
        .arg(to)
        .args(extra)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(ContentError::PandocSpawn)?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(input.as_bytes())
            .map_err(ContentError::PandocSpawn)?;
    }
    let output = child
        .wait_with_output()
        .map_err(ContentError::PandocSpawn)?;
    if !output.status.success() {
        return Err(ContentError::Pandoc {
            from: from.to_owned(),
            to: to.to_owned(),
            status: output.status.to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}
