//! Chat-service (IC3) operations: conversations, messages, threads, reactions.

use eyre::Result;
use reqwest::Method;
use serde_json::json;

use super::ApiClient;
use crate::model::{Conversation, ConversationsResponse, Message, MessagesResponse, Thread};

pub async fn list_conversations(client: &ApiClient, limit: u32) -> Result<Vec<Conversation>> {
    let limit = limit.to_string();
    let request = client.chat(Method::GET, "conversations").query(&[
        ("view", "msnp24Equivalent"),
        ("pageSize", limit.as_str()),
        ("startTime", "1"),
    ]);
    let resp = client.exec(request, "GET conversations").await?;
    Ok(resp.json::<ConversationsResponse>().await?.conversations)
}

pub async fn get_messages(client: &ApiClient, conversation: &str, limit: u32) -> Result<Vec<Message>> {
    let limit = limit.to_string();
    let path = format!("conversations/{}/messages", encode(conversation));
    let request = client
        .chat(Method::GET, &path)
        .query(&[("pageSize", limit.as_str()), ("startTime", "1")]);
    let resp = client.exec(request, "GET messages").await?;
    Ok(resp.json::<MessagesResponse>().await?.messages)
}

pub async fn get_thread(client: &ApiClient, conversation: &str, root: &str, limit: u32) -> Result<Vec<Message>> {
    get_messages(client, &thread_target(conversation, Some(root)), limit).await
}

pub async fn get_message(client: &ApiClient, conversation: &str, message: &str) -> Result<Message> {
    let path = format!("conversations/{}/messages/{message}", encode(conversation));
    let request = client.chat(Method::GET, &path);
    let resp = client.exec(request, "GET message").await?;
    Ok(resp.json::<Message>().await?)
}

pub async fn list_threads(
    client: &ApiClient,
    conversation: &str,
    limit: u32,
    all_replies: bool,
) -> Result<Vec<Thread>> {
    let scan = limit.saturating_mul(3).clamp(50, 200);
    let mut roots: Vec<Message> = get_messages(client, conversation, scan)
        .await?
        .into_iter()
        .filter(Message::is_thread_root)
        .take(limit as usize)
        .collect();
    roots.sort_by(|a, b| a.time_key().cmp(b.time_key()));
    let mut threads = Vec::with_capacity(roots.len());
    for root in roots {
        let replies = if all_replies {
            reply_messages(client, conversation, &root).await?
        } else {
            Vec::new()
        };
        threads.push(Thread { root, replies });
    }
    Ok(threads)
}

async fn reply_messages(client: &ApiClient, conversation: &str, root: &Message) -> Result<Vec<Message>> {
    let Some(root_id) = root.id.as_deref() else {
        return Ok(Vec::new());
    };
    let mut messages = get_thread(client, conversation, root_id, 200).await?;
    messages.retain(|m| m.id.as_deref() != Some(root_id));
    messages.sort_by(|a, b| a.time_key().cmp(b.time_key()));
    Ok(messages)
}

pub async fn post_message(
    client: &ApiClient,
    conversation: &str,
    reply_to: Option<&str>,
    text: &str,
    html: bool,
) -> Result<String> {
    let target = thread_target(conversation, reply_to);
    let path = format!("conversations/{}/messages", encode(&target));
    let request = client.chat(Method::POST, &path).json(&compose_body(client, text, html, None));
    let resp = client.exec(request, "POST message").await?;
    Ok(extract_message_id(resp).await)
}

pub async fn edit_message(
    client: &ApiClient,
    conversation: &str,
    message: &str,
    text: &str,
    html: bool,
) -> Result<()> {
    let path = format!("conversations/{}/messages/{message}", encode(conversation));
    let request = client.chat(Method::PUT, &path).json(&compose_body(client, text, html, Some(message)));
    client.exec(request, "PUT edit").await?;
    Ok(())
}

pub async fn add_reaction(client: &ApiClient, conversation: &str, message: &str, emoji: &str) -> Result<()> {
    let path = format!("conversations/{}/messages/{message}/properties", encode(conversation));
    let body = json!({ "emotions": { "key": emoji, "value": now_millis() } });
    let request = client.chat(Method::PUT, &path).query(&[("name", "emotions")]).json(&body);
    client.exec(request, "PUT reaction").await?;
    Ok(())
}

fn encode(conversation: &str) -> String {
    urlencoding::encode(conversation).into_owned()
}

fn thread_target(conversation: &str, root: Option<&str>) -> String {
    match root {
        Some(r) => format!("{conversation};messageid={r}"),
        None => conversation.to_owned(),
    }
}

fn compose_body(client: &ApiClient, text: &str, html: bool, edit_id: Option<&str>) -> serde_json::Value {
    let name = client.session().identity.name.clone().unwrap_or_default();
    let content = if html { text.to_owned() } else { plain_to_html(text) };
    let mut body = json!({
        "content": content,
        "messagetype": "RichText/Html",
        "contenttype": "text",
        "imdisplayname": name,
    });
    match edit_id {
        Some(id) => body["skypeeditedid"] = json!(id),
        None => body["clientmessageid"] = json!(now_nanos()),
    }
    body
}

fn plain_to_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\n' => out.push_str("<br>"),
            other => out.push(other),
        }
    }
    out
}

async fn extract_message_id(resp: reqwest::Response) -> String {
    if let Some(id) = resp
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|loc| loc.rsplit('/').find(|s| !s.is_empty()))
        .map(str::to_owned)
    {
        return id;
    }
    let value: serde_json::Value = resp.json().await.unwrap_or(serde_json::Value::Null);
    value
        .get("OriginalArrivalTime")
        .map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_owned))
        .unwrap_or_default()
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

fn now_nanos() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos().to_string())
        .unwrap_or_else(|_| "0".to_owned())
}
