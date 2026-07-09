//! `message` commands.

use std::path::Path;

use eyre::{Result, eyre};

use crate::api::{ApiClient, chat};
use crate::auth::AuthInteraction;
use crate::cli::{
    MessageEditArgs, MessageListArgs, MessageNewArgs, MessageReactArgs, MessageRefArgs, MessageVerb,
};
use crate::link::resolve_conversation;
use crate::model::{Message, MessageAction};
use crate::output::{MessageList, render};

pub async fn dispatch(verb: MessageVerb, cookies: Option<&Path>, json: bool) -> Result<()> {
    let client = ApiClient::connect(cookies, AuthInteraction::from_json(json)).await?;
    match verb {
        MessageVerb::New(args) => render(&new(&client, args).await?, json),
        MessageVerb::List(args) => render(&list(&client, args).await?, json),
        MessageVerb::Read(args) => render(&read(&client, args).await?, json),
        MessageVerb::Edit(args) => render(&edit(&client, args).await?, json),
        MessageVerb::React(args) => render(&react(&client, args).await?, json),
    }
}

async fn new(client: &ApiClient, args: MessageNewArgs) -> Result<MessageAction> {
    let (conversation, link) = resolve_conversation(args.conversation);
    let reply_to = args.reply_to.or_else(|| link.and_then(|l| l.thread_ref().map(str::to_owned)));
    let id = chat::post_message(client, &conversation, reply_to.as_deref(), &args.text, args.html)
        .await?;
    Ok(MessageAction { action: "posted", conversation, message_id: id, emoji: None })
}

async fn list(client: &ApiClient, args: MessageListArgs) -> Result<MessageList> {
    let (conversation, _) = resolve_conversation(args.conversation);
    Ok(MessageList::new(chat::get_messages(client, &conversation, args.limit).await?))
}

async fn read(client: &ApiClient, args: MessageRefArgs) -> Result<Message> {
    let (conversation, link) = resolve_conversation(args.conversation);
    let message = args
        .message
        .or_else(|| link.and_then(|l| l.message_ref().map(str::to_owned)))
        .ok_or_else(|| eyre!("no message id: pass one, or use a message link"))?;
    chat::get_message(client, &conversation, &message).await
}

async fn edit(client: &ApiClient, args: MessageEditArgs) -> Result<MessageAction> {
    let (conversation, link) = resolve_conversation(args.conversation);
    let link_message = link.and_then(|l| l.message_ref().map(str::to_owned));
    let (message, text) =
        resolve_id_and_trailing(link_message, args.message, args.text, "text")?;
    chat::edit_message(client, &conversation, &message, &text, args.html).await?;
    Ok(MessageAction { action: "edited", conversation, message_id: message, emoji: None })
}

async fn react(client: &ApiClient, args: MessageReactArgs) -> Result<MessageAction> {
    let (conversation, link) = resolve_conversation(args.conversation);
    let link_message = link.and_then(|l| l.message_ref().map(str::to_owned));
    let (message, emoji) =
        resolve_id_and_trailing(link_message, args.message, args.emoji, "emoji")?;
    chat::add_reaction(client, &conversation, &message, &emoji).await?;
    Ok(MessageAction { action: "reacted", conversation, message_id: message, emoji: Some(emoji) })
}

fn resolve_id_and_trailing(
    link_message: Option<String>,
    first: Option<String>,
    second: Option<String>,
    trailing: &str,
) -> Result<(String, String)> {
    match (link_message, first, second) {
        (_, Some(id), Some(value)) => Ok((id, value)),
        (Some(id), Some(value), None) => Ok((id, value)),
        (Some(_), None, _) => Err(eyre!("missing {trailing} (after the message link)")),
        (None, _, _) => {
            Err(eyre!("missing message id and/or {trailing}: pass both, or use a message link"))
        }
    }
}
