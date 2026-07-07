//! Serde types for chat-service responses (parsed at the boundary).

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct ConversationsResponse {
    #[serde(default)]
    pub conversations: Vec<Conversation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    pub id: String,
    #[serde(rename = "threadProperties", default)]
    pub thread_properties: Option<ThreadProperties>,
    #[serde(rename = "lastMessage", default)]
    pub last_message: Option<LastMessage>,
}

impl Conversation {
    pub fn is_channel(&self) -> bool {
        self.id.contains("@thread.tacv2")
    }

    pub fn topic(&self) -> &str {
        self.thread_properties.as_ref().and_then(|t| t.topic.as_deref()).unwrap_or("")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadProperties {
    #[serde(default)]
    pub topic: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LastMessage {
    #[serde(default)]
    pub content: Option<String>,
    #[serde(rename = "imdisplayname", default)]
    pub im_display_name: Option<String>,
    #[serde(rename = "composetime", default)]
    pub compose_time: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct MessagesResponse {
    #[serde(default)]
    pub messages: Vec<Message>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(rename = "originalarrivaltime", default)]
    pub original_arrival_time: Option<String>,
    #[serde(rename = "composetime", default)]
    pub compose_time: Option<String>,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(rename = "imdisplayname", default)]
    pub im_display_name: Option<String>,
    #[serde(rename = "messagetype", default)]
    pub message_type: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AuthStatus {
    pub user: Option<String>,
    pub name: Option<String>,
    pub tenant: Option<String>,
    pub audience: Option<String>,
    pub region: String,
    pub chat_service: String,
    pub token_ttl_min: Option<i64>,
    pub services: usize,
}

#[derive(Debug, Serialize)]
pub struct MessageAction {
    pub action: &'static str,
    pub conversation: String,
    pub message_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub emoji: Option<String>,
}
