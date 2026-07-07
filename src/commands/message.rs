//! `message` commands.

use std::path::Path;

use anyhow::Result;

use crate::api::{ApiClient, chat};
use crate::cli::{
    MessageEditArgs, MessageListArgs, MessageNewArgs, MessageReactArgs, MessageRefArgs, MessageVerb,
};
use crate::model::{Message, MessageAction};
use crate::output::{MessageList, render};

pub async fn dispatch(verb: MessageVerb, cookies: Option<&Path>, json: bool) -> Result<()> {
    let client = ApiClient::connect(cookies).await?;
    match verb {
        MessageVerb::New(args) => render(&new(&client, args).await?, json),
        MessageVerb::List(args) => render(&list(&client, args).await?, json),
        MessageVerb::Read(args) => render(&read(&client, args).await?, json),
        MessageVerb::Edit(args) => render(&edit(&client, args).await?, json),
        MessageVerb::React(args) => render(&react(&client, args).await?, json),
    }
}

async fn new(client: &ApiClient, args: MessageNewArgs) -> Result<MessageAction> {
    let id =
        chat::post_message(client, &args.conversation, args.reply_to.as_deref(), &args.text, args.html)
            .await?;
    Ok(MessageAction { action: "posted", conversation: args.conversation, message_id: id, emoji: None })
}

async fn list(client: &ApiClient, args: MessageListArgs) -> Result<MessageList> {
    Ok(MessageList(chat::get_messages(client, &args.conversation, args.limit).await?))
}

async fn read(client: &ApiClient, args: MessageRefArgs) -> Result<Message> {
    chat::get_message(client, &args.conversation, &args.message).await
}

async fn edit(client: &ApiClient, args: MessageEditArgs) -> Result<MessageAction> {
    chat::edit_message(client, &args.conversation, &args.message, &args.text, args.html).await?;
    Ok(MessageAction {
        action: "edited",
        conversation: args.conversation,
        message_id: args.message,
        emoji: None,
    })
}

async fn react(client: &ApiClient, args: MessageReactArgs) -> Result<MessageAction> {
    chat::add_reaction(client, &args.conversation, &args.message, &args.emoji).await?;
    Ok(MessageAction {
        action: "reacted",
        conversation: args.conversation,
        message_id: args.message,
        emoji: Some(args.emoji),
    })
}
