//! Output rendering. Commands return data values; this is the only module that
//! writes them out — as human text (`DisplayOutput`) or JSON (`serde`). Future
//! color/table modes extend here without touching business logic.

use eyre::Result;
use serde::Serialize;

use crate::model::{
    AuthAction, AuthStatus, CalendarEvent, Conversation, Message, MessageAction, Person, Team,
    Thread, TokenInfo,
};

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
pub struct MessageList(Vec<Message>);

impl MessageList {
    /// Stores messages earliest-first so the JSON (`serde(transparent)`) and human
    /// renderings share one chronological order; the display filter only hides
    /// empty/system content, it never reorders.
    pub fn new(mut messages: Vec<Message>) -> Self {
        messages.sort_by(|a, b| a.time_key().cmp(b.time_key()));
        Self(messages)
    }
}

impl DisplayOutput for MessageList {
    fn display_output(&self) -> String {
        self.0
            .iter()
            .filter(|m| !html_to_text(m.content.as_deref().unwrap_or("")).is_empty())
            .map(|m| m.display_output())
            .collect::<Vec<_>>()
            .join("\n")
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

impl DisplayOutput for Person {
    fn display_output(&self) -> String {
        let name = self.display_name.as_deref().unwrap_or("(unknown)");
        let mut line = name.to_owned();
        if let Some(email) = self.email_addresses.first() {
            line.push_str(&format!("  <{email}>"));
        }
        if let Some(title) = self.job_title.as_deref().filter(|t| !t.is_empty()) {
            line.push_str(&format!("  — {title}"));
        }
        line
    }
}

impl DisplayOutput for Team {
    fn display_output(&self) -> String {
        let count = self.channels.len();
        let plural = if count == 1 { "" } else { "s" };
        format!("{}\n    {}  ({count} channel{plural})", self.id, self.display_name)
    }
}

impl DisplayOutput for CalendarEvent {
    fn display_output(&self) -> String {
        let when = self.start.as_ref().and_then(|s| s.date_time.as_deref()).unwrap_or("");
        let subject = self.subject.as_deref().unwrap_or("(no subject)");
        let mut line = format!("[{when}] {subject}");
        if let Some(name) =
            self.organizer.as_ref().and_then(|o| o.email_address.as_ref()).and_then(|e| e.name.as_deref())
        {
            line.push_str(&format!("  — {name}"));
        }
        if let Some(place) =
            self.location.as_ref().and_then(|l| l.display_name.as_deref()).filter(|p| !p.is_empty())
        {
            line.push_str(&format!("  @ {place}"));
        }
        if self.is_online_meeting {
            line.push_str("  (online)");
        }
        if self.is_cancelled {
            line.push_str("  [CANCELLED]");
        }
        line
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

impl DisplayOutput for AuthAction {
    fn display_output(&self) -> String {
        if self.signed_in { "Signed in.".to_owned() } else { "Signed out.".to_owned() }
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
        lines.push(format!("Services  : {} region endpoints", self.services));
        lines.push("Tokens    :".to_owned());
        for token in &self.tokens {
            lines.push(format!("  {}", token.display_output()));
        }
        lines.join("\n")
    }
}

impl DisplayOutput for TokenInfo {
    fn display_output(&self) -> String {
        let aud = self.audience.as_deref().unwrap_or("-");
        let ttl = self.expires_in_min.map_or_else(|| "n/a".to_owned(), |min| format!("{min} min"));
        let mut line = format!("{:16} aud={aud}  exp={ttl}", self.name);
        if let Some(value) = &self.value {
            line.push_str(&format!("  token={value}"));
        }
        line
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::model::Message;

    fn msg(id: &str, time: &str) -> Message {
        Message {
            id: Some(id.to_owned()),
            original_arrival_time: Some(time.to_owned()),
            compose_time: None,
            content: Some(format!("<p>{id}</p>")),
            im_display_name: Some("tester".to_owned()),
            message_type: Some("RichText/Html".to_owned()),
            root_message_id: None,
            sequence_id: None,
        }
    }

    #[test]
    fn message_list_is_earliest_first_in_json_and_human() {
        let list = MessageList::new(vec![
            msg("c", "2026-01-03T00:00:00.0000000Z"),
            msg("a", "2026-01-01T00:00:00.0000000Z"),
            msg("b", "2026-01-02T00:00:00.0000000Z"),
        ]);

        let json = serde_json::to_string(&list).unwrap();
        let (ja, jb, jc) =
            (json.find("\"a\"").unwrap(), json.find("\"b\"").unwrap(), json.find("\"c\"").unwrap());
        assert!(ja < jb && jb < jc, "JSON order must be earliest-first: {json}");

        let text = list.display_output();
        let (ta, tb, tc) =
            (text.find("(a)").unwrap(), text.find("(b)").unwrap(), text.find("(c)").unwrap());
        assert!(ta < tb && tb < tc, "human order must be earliest-first: {text}");
    }
}
