//! `chat` commands (1:1 and group conversations; channels are excluded here).

use std::path::Path;

use eyre::Result;

use crate::api::{ApiClient, chat};
use crate::auth::AuthInteraction;
use crate::cli::ChatVerb;
use crate::model::Conversation;
use crate::output::render;

pub async fn dispatch(verb: ChatVerb, cookies: Option<&Path>, json: bool) -> Result<()> {
    let client = ApiClient::connect(cookies, AuthInteraction::from_json(json)).await?;
    match verb {
        ChatVerb::List(args) => {
            let chats: Vec<Conversation> = chat::list_conversations(&client, args.limit)
                .await?
                .into_iter()
                .filter(|conversation| !conversation.is_channel())
                .collect();
            render(&chats, json)
        }
    }
}
