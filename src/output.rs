//! Output rendering. Commands return data values; this is the only module that
//! writes them out — as human text (`DisplayOutput`) or JSON (`serde`). Future
//! color/table modes extend here without touching business logic.

use eyre::Result;
use serde::Serialize;

use crate::model::{AuthStatus, Conversation, Message, MessageAction, Thread};

pub trait DisplayOutput {
    fn display_output(&self) -> String;
}

impl<T: DisplayOutput> DisplayOutput for Vec<T> {
    fn display_output(&self) -> String {
        self.iter().map(DisplayOutput::display_output).collect::<Vec<_>>().join("\n")
    }
}

pub fn render<T: Serialize + DisplayOutput>(value: &T, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(value)?);
    } else {
        let text = value.display_output();
        if text.is_empty() {
            eprintln!("(no results)");
        } else {
            println!("{text}");
        }
    }
    Ok(())
}

#[derive(Debug, Serialize)]
#[serde(transparent)]
pub struct MessageList(pub Vec<Message>);

impl DisplayOutput for MessageList {
    fn display_output(&self) -> String {
        let mut visible: Vec<&Message> = self
            .0
            .iter()
            .filter(|m| !html_to_text(m.content.as_deref().unwrap_or("")).is_empty())
            .collect();
        visible.sort_by(|a, b| a.time_key().cmp(b.time_key()));
        visible.iter().map(|m| m.display_output()).collect::<Vec<_>>().join("\n")
    }
}

#[derive(Debug, Serialize)]
#[serde(transparent)]
pub struct ThreadList(pub Vec<Thread>);

impl DisplayOutput for ThreadList {
    fn display_output(&self) -> String {
        let sep = if self.0.iter().any(|t| !t.replies.is_empty()) { "\n\n" } else { "\n" };
        self.0.iter().map(DisplayOutput::display_output).collect::<Vec<_>>().join(sep)
    }
}

impl DisplayOutput for Thread {
    fn display_output(&self) -> String {
        let mut out = self.root.display_output();
        for reply in &self.replies {
            out.push('\n');
            out.push_str(&indent(&reply.display_output(), "    "));
        }
        out
    }
}

impl DisplayOutput for Conversation {
    fn display_output(&self) -> String {
        let topic = self.thread_properties.as_ref().and_then(|t| t.topic.as_deref()).unwrap_or("");
        let preview: String = self
            .last_message
            .as_ref()
            .and_then(|m| m.content.as_deref())
            .map(html_to_text)
            .unwrap_or_default()
            .chars()
            .take(60)
            .collect();
        format!("{}\n    {topic}  {preview}", self.id)
    }
}

impl DisplayOutput for Message {
    fn display_output(&self) -> String {
        let text = html_to_text(self.content.as_deref().unwrap_or(""));
        let who = self.im_display_name.as_deref().unwrap_or("?");
        let when = self.compose_time.as_deref().or(self.original_arrival_time.as_deref()).unwrap_or("");
        let id = self.id.as_deref().unwrap_or("");
        format!("[{when}] {who} ({id}): {text}")
    }
}

impl DisplayOutput for MessageAction {
    fn display_output(&self) -> String {
        match &self.emoji {
            Some(emoji) => {
                format!("{} {emoji} on message {} in {}", self.action, self.message_id, self.conversation)
            }
            None => format!("{} message {} in {}", self.action, self.message_id, self.conversation),
        }
    }
}

impl DisplayOutput for AuthStatus {
    fn display_output(&self) -> String {
        let mut lines = vec![format!(
            "Signed in : {}",
            self.user.as_deref().or(self.name.as_deref()).unwrap_or("<unknown>")
        )];
        if let Some(name) = &self.name {
            lines.push(format!("Name      : {name}"));
        }
        if let Some(tenant) = &self.tenant {
            lines.push(format!("Tenant    : {tenant}"));
        }
        lines.push(format!("Region    : {}", self.region));
        lines.push(format!("Chat svc  : {}", self.chat_service));
        if let Some(audience) = &self.audience {
            lines.push(format!("AAD aud   : {audience}"));
        }
        if let Some(ttl) = self.token_ttl_min {
            lines.push(format!("Token TTL : {ttl} min"));
        }
        lines.push(format!("Services  : {} region endpoints", self.services));
        lines.join("\n")
    }
}

fn indent(text: &str, prefix: &str) -> String {
    text.lines().map(|line| format!("{prefix}{line}")).collect::<Vec<_>>().join("\n")
}

fn html_to_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            other if !in_tag => out.push(other),
            _ => {}
        }
    }
    out.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        .trim()
        .to_owned()
}
