//! `message` commands.

use std::io::Read;
use std::path::Path;

use eyre::{Result, WrapErr, eyre};

use crate::api::{ApiClient, chat, substrate};
use crate::auth::{self, AuthInteraction};
use crate::cli::{
    ContentInputArgs, MessageEditArgs, MessageListArgs, MessageNewArgs, MessageReactArgs,
    MessageRefArgs, MessageVerb,
};
use crate::content;
use crate::content::mentions::{self, Mention, MentionSpec};
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
        MessageVerb::New(args) => render(&new(&client, args, json, pandoc).await?, json),
        MessageVerb::List(args) => render(&list(&client, args, json, pandoc).await?, json),
        MessageVerb::Read(args) => render(&read(&client, args, json, pandoc).await?, json),
        MessageVerb::Edit(args) => render(&edit(&client, args, json, pandoc).await?, json),
        MessageVerb::React(args) => render(&react(&client, args).await?, json),
    }
}

async fn new(
    client: &ApiClient,
    args: MessageNewArgs,
    json: bool,
    pandoc: &[String],
) -> Result<MessageAction> {
    let (conversation, link) = resolve_conversation(args.conversation);
    let reply_to = args
        .reply_to
        .or_else(|| link.and_then(|l| l.thread_ref().map(str::to_owned)));
    let (html, mentions) = build_wire_html(client, args.content, json, pandoc).await?;
    let id =
        chat::post_message(client, &conversation, reply_to.as_deref(), &html, &mentions).await?;
    Ok(MessageAction {
        action: "posted",
        conversation,
        message_id: id,
        emoji: None,
    })
}

async fn list(
    client: &ApiClient,
    args: MessageListArgs,
    json: bool,
    pandoc: &[String],
) -> Result<MessageList> {
    let (conversation, _) = resolve_conversation(args.conversation);
    let format = content::resolve_output(
        args.content.content_output_format,
        args.content.content_format,
        json,
    )?;
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
    let format = content::resolve_output(
        args.content.content_output_format,
        args.content.content_format,
        json,
    )?;
    let mut message = chat::get_message(client, &conversation, &message).await?;
    content::apply_output(&mut message, &format, pandoc)?;
    Ok(message)
}

async fn edit(
    client: &ApiClient,
    args: MessageEditArgs,
    json: bool,
    pandoc: &[String],
) -> Result<MessageAction> {
    let (conversation, link) = resolve_conversation(args.conversation);
    let message = require_message_id(args.message, link)?;
    let (html, mentions) = build_wire_html(client, args.content, json, pandoc).await?;
    chat::edit_message(client, &conversation, &message, &html, &mentions).await?;
    Ok(MessageAction {
        action: "edited",
        conversation,
        message_id: message,
        emoji: None,
    })
}

async fn react(client: &ApiClient, args: MessageReactArgs) -> Result<MessageAction> {
    let (conversation, link) = resolve_conversation(args.conversation);
    let link_message = link.and_then(|l| l.message_ref().map(str::to_owned));
    let (message, emoji) = resolve_react_target(link_message, args.message, args.emoji)?;
    chat::add_reaction(client, &conversation, &message, &emoji).await?;
    Ok(MessageAction {
        action: "reacted",
        conversation,
        message_id: message,
        emoji: Some(emoji),
    })
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
/// options: read the body (`--content` or stdin), convert it per `-I/-f`, then
/// resolve any `@{…}` mention tokens into mention spans + metadata.
async fn build_wire_html(
    client: &ApiClient,
    args: ContentInputArgs,
    json: bool,
    pandoc: &[String],
) -> Result<(String, Vec<Mention>)> {
    let format = content::resolve_input(args.content_input_format, args.content_format)?;
    let body = read_content(args.content)?;
    let html = content::to_teams_html(&body, &format, pandoc)?;
    let doc = mentions::parse(&html)?;
    let resolved = resolve_mentions(client, doc.specs(), json).await?;
    Ok(doc.assemble(&resolved))
}

/// Resolve mention specs to `(mri, display name)` pairs. Explicit `@{mri|name}`
/// tokens need no lookup; `@{#channel}` tokens match against the chat-service
/// conversation list (the same source as `channel list`, any auth world);
/// `@{query}` person tokens go through people search, which needs a signed-in
/// authenticator (loaded upfront so a missing login fails before any lookup).
async fn resolve_mentions(
    client: &ApiClient,
    specs: &[MentionSpec],
    json: bool,
) -> Result<Vec<(String, String)>> {
    const SEARCH_LIMIT: u32 = 10;
    const CHANNEL_SCAN_LIMIT: u32 = 200;
    let authenticator = if specs.iter().any(|s| matches!(s, MentionSpec::Query(_))) {
        Some(
            auth::load_authenticator(AuthInteraction::from_json(json)).wrap_err(
                "resolving @{…} person mentions needs people search (run `xteams auth login`), \
             or supply the MRI: @{<mri>|<Display Name>}",
            )?,
        )
    } else {
        None
    };
    let mut channels = None;
    let mut resolved = Vec::with_capacity(specs.len());
    for spec in specs {
        resolved.push(match spec {
            MentionSpec::Explicit { mri, name } => (mri.clone(), name.clone()),
            MentionSpec::Query(query) => {
                let auth = authenticator
                    .as_ref()
                    .ok_or_else(|| eyre!("mention lookup unavailable"))?;
                let people = substrate::search_people(auth, query, SEARCH_LIMIT).await?;
                mentions::pick_person(query, &people)?
            }
            MentionSpec::ChannelQuery(query) => {
                let known = match &mut channels {
                    Some(known) => known,
                    slot => {
                        slot.insert(chat::list_conversations(client, CHANNEL_SCAN_LIMIT).await?)
                    }
                };
                mentions::pick_channel(query, known)?
            }
        });
    }
    Ok(resolved)
}

/// Resolve a required message id from an explicit argument, else from a message
/// link.
fn require_message_id(
    explicit: Option<String>,
    link: Option<TeamsDeepLinkFields>,
) -> Result<String> {
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
        (None, _, _) => Err(eyre!(
            "missing message id and/or emoji: pass both, or use a message link"
        )),
    }
}
