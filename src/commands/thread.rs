//! `thread` commands.

use std::path::Path;

use eyre::Result;

use crate::api::{ApiClient, chat};
use crate::cli::ThreadVerb;
use crate::output::{MessageList, ThreadList, render};

pub async fn dispatch(verb: ThreadVerb, cookies: Option<&Path>, json: bool) -> Result<()> {
    let client = ApiClient::connect(cookies).await?;
    match verb {
        ThreadVerb::List(args) => {
            let threads =
                chat::list_threads(&client, &args.conversation, args.limit, args.all_replies).await?;
            render(&ThreadList(threads), json)
        }
        ThreadVerb::Read(args) => {
            let messages = chat::get_thread(&client, &args.conversation, &args.message, 200).await?;
            render(&MessageList(messages), json)
        }
    }
}
