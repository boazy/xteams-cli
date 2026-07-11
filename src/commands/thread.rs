//! `thread` commands.

use std::path::Path;

use eyre::{Result, eyre};

use crate::api::{ApiClient, chat};
use crate::auth::AuthInteraction;
use crate::cli::ThreadVerb;
use crate::content;
use crate::link::resolve_conversation;
use crate::output::{MessageList, ThreadList, render};

pub async fn dispatch(
    verb: ThreadVerb,
    cookies: Option<&Path>,
    json: bool,
    pandoc: &[String],
) -> Result<()> {
    let client = ApiClient::connect(cookies, AuthInteraction::from_json(json)).await?;
    match verb {
        ThreadVerb::List(args) => {
            let (conversation, _) = resolve_conversation(args.conversation);
            let format = content::resolve_output(
                args.content.content_output_format,
                args.content.content_format,
                json,
            )?;
            let mut threads =
                chat::list_threads(&client, &conversation, args.limit, args.all_replies).await?;
            for thread in &mut threads {
                content::apply_output(&mut thread.root, &format, pandoc)?;
                for reply in &mut thread.replies {
                    content::apply_output(reply, &format, pandoc)?;
                }
            }
            render(&ThreadList(threads), json)
        }
        ThreadVerb::Read(args) => {
            let (conversation, link) = resolve_conversation(args.conversation);
            let root = args
                .message
                .or_else(|| link.and_then(|l| l.thread_ref().map(str::to_owned)))
                .ok_or_else(|| eyre!("no thread root id: pass one, or use a message link"))?;
            let format = content::resolve_output(
                args.content.content_output_format,
                args.content.content_format,
                json,
            )?;
            let mut messages = chat::get_thread(&client, &conversation, &root, 200).await?;
            for message in &mut messages {
                content::apply_output(message, &format, pandoc)?;
            }
            render(&MessageList::new(messages), json)
        }
    }
}
