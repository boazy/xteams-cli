//! `channel` commands. Channels are read from the chat-service conversation list
//! (the channels you follow); `list <team>` / `search` filter by name substring.
//! Full team-scoped enumeration needs a chatsvcagg token (deferred).

use std::path::Path;

use eyre::Result;

use crate::api::{ApiClient, chat};
use crate::auth::AuthInteraction;
use crate::cli::ChannelVerb;
use crate::model::Conversation;
use crate::output::render;

const SCAN_LIMIT: u32 = 200;

pub async fn dispatch(verb: ChannelVerb, cookies: Option<&Path>, json: bool) -> Result<()> {
    let client = ApiClient::connect(cookies, AuthInteraction::from_json(json)).await?;
    let channels: Vec<Conversation> = chat::list_conversations(&client, SCAN_LIMIT)
        .await?
        .into_iter()
        .filter(Conversation::is_channel)
        .collect();
    let result: Vec<Conversation> = match verb {
        ChannelVerb::List(args) => match args.team {
            Some(team) => channels.into_iter().filter(|c| matches(c, &team)).collect(),
            None => channels,
        },
        ChannelVerb::Search(args) => channels
            .into_iter()
            .filter(|c| matches(c, &args.query))
            .collect(),
    };
    render(&result, json)
}

fn matches(conversation: &Conversation, query: &str) -> bool {
    let needle = query.to_lowercase();
    conversation.topic().to_lowercase().contains(&needle)
        || conversation.id.to_lowercase().contains(&needle)
}
