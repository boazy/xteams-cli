//! `message` commands.

use std::io::Read;
use std::path::Path;

use eyre::{Result, eyre};

use crate::api::{ApiClient, chat};
use crate::auth::AuthInteraction;
use crate::cli::{
    ContentInputArgs, MessageEditArgs, MessageListArgs, MessageNewArgs, MessageReactArgs,
    MessageRefArgs, MessageVerb,
};
use crate::content;
use crate::link::{TeamsDeepLinkFields, resolve_conversation};
use crate::model::{Message, MessageAction};
use crate::output::{MessageList, render};

pub async fn dispatch(
    verb: MessageVerb,
    cookies: Option<&Path>,
    json: bool,
    pandoc: &[String],
) -> Result<()> {
    let client = ApiClient::connect(cookies, AuthInteraction::from_json(json)).await?;
    match verb {
        MessageVerb::New(args) => render(&new(&client, args, pandoc).await?, json),
        MessageVerb::List(args) => render(&list(&client, args, json, pandoc).await?, json),
        MessageVerb::Read(args) => render(&read(&client, args, json, pandoc).await?, json),
        MessageVerb::Edit(args) => render(&edit(&client, args, pandoc).await?, json),
        MessageVerb::React(args) => render(&react(&client, args).await?, json),
    }
}

async fn new(client: &ApiClient, args: MessageNewArgs, pandoc: &[String]) -> Result<MessageAction> {
    let (conversation, link) = resolve_conversation(args.conversation);
    let reply_to = args.reply_to.or_else(|| link.and_then(|l| l.thread_ref().map(str::to_owned)));
    let html = build_wire_html(args.content, pandoc)?;
    let id = chat::post_message(client, &conversation, reply_to.as_deref(), &html).await?;
    Ok(MessageAction { action: "posted", conversation, message_id: id, emoji: None })
}

async fn list(
    client: &ApiClient,
    args: MessageListArgs,
    json: bool,
    pandoc: &[String],
) -> Result<MessageList> {
    let (conversation, _) = resolve_conversation(args.conversation);
    let format =
        content::resolve_output(args.content.content_output_format, args.content.content_format, json)?;
    let mut messages = chat::get_messages(client, &conversation, args.limit).await?;
    for message in &mut messages {
        content::apply_output(message, &format, pandoc)?;
    }
    Ok(MessageList::new(messages))
}

async fn read(
    client: &ApiClient,
    args: MessageRefArgs,
    json: bool,
    pandoc: &[String],
) -> Result<Message> {
    let (conversation, link) = resolve_conversation(args.conversation);
    let message = require_message_id(args.message, link)?;
    let format =
        content::resolve_output(args.content.content_output_format, args.content.content_format, json)?;
    let mut message = chat::get_message(client, &conversation, &message).await?;
    content::apply_output(&mut message, &format, pandoc)?;
    Ok(message)
}

async fn edit(
    client: &ApiClient,
    args: MessageEditArgs,
    pandoc: &[String],
) -> Result<MessageAction> {
    let (conversation, link) = resolve_conversation(args.conversation);
    let message = require_message_id(args.message, link)?;
    let html = build_wire_html(args.content, pandoc)?;
    chat::edit_message(client, &conversation, &message, &html).await?;
    Ok(MessageAction { action: "edited", conversation, message_id: message, emoji: None })
}

async fn react(client: &ApiClient, args: MessageReactArgs) -> Result<MessageAction> {
    let (conversation, link) = resolve_conversation(args.conversation);
    let link_message = link.and_then(|l| l.message_ref().map(str::to_owned));
    let (message, emoji) = resolve_react_target(link_message, args.message, args.emoji)?;
    chat::add_reaction(client, &conversation, &message, &emoji).await?;
    Ok(MessageAction { action: "reacted", conversation, message_id: message, emoji: Some(emoji) })
}

/// Read message content: the `--content` value, or stdin (verbatim) when it is
/// omitted.
fn read_content(content: Option<String>) -> Result<String> {
    match content {
        Some(value) => Ok(value),
        None => {
            let mut buf = String::new();
            std::io::stdin().read_to_string(&mut buf)?;
            Ok(buf)
        }
    }
}

/// Build the HTML Teams stores on the wire from `message new/edit` content
/// options: read the body (`--content` or stdin) and convert it per `-I/-f`.
fn build_wire_html(args: ContentInputArgs, pandoc: &[String]) -> Result<String> {
    let format = content::resolve_input(args.content_input_format, args.content_format)?;
    let body = read_content(args.content)?;
    Ok(content::to_teams_html(&body, &format, pandoc)?)
}

/// Resolve a required message id from an explicit argument, else from a message
/// link.
fn require_message_id(explicit: Option<String>, link: Option<TeamsDeepLinkFields>) -> Result<String> {
    explicit
        .or_else(|| link.and_then(|l| l.message_ref().map(str::to_owned)))
        .ok_or_else(|| eyre!("no message id: pass one, or use a message link"))
}

/// Resolve `react`'s (message id, emoji). A message link can supply the id, in
/// which case the sole positional is the emoji.
fn resolve_react_target(
    link_message: Option<String>,
    message: Option<String>,
    emoji: Option<String>,
) -> Result<(String, String)> {
    match (link_message, message, emoji) {
        (_, Some(id), Some(emoji)) => Ok((id, emoji)),
        (Some(id), Some(emoji), None) => Ok((id, emoji)),
        (Some(_), None, _) => Err(eyre!("missing emoji (after the message link)")),
        (None, _, _) => {
            Err(eyre!("missing message id and/or emoji: pass both, or use a message link"))
        }
    }
}
