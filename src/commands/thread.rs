//! `thread` commands.

use std::path::Path;

use anyhow::Result;

use crate::api::{ApiClient, chat};
use crate::cli::ThreadVerb;
use crate::output::{MessageList, render};

pub async fn dispatch(verb: ThreadVerb, cookies: Option<&Path>, json: bool) -> Result<()> {
    let client = ApiClient::connect(cookies).await?;
    match verb {
        ThreadVerb::List(args) => {
            let messages = chat::get_thread(&client, &args.conversation, &args.message, 200).await?;
            render(&MessageList(messages), json)
        }
    }
}
