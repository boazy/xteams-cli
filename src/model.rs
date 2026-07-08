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
    #[serde(rename = "rootMessageId", default)]
    pub root_message_id: Option<String>,
    #[serde(rename = "sequenceId", default)]
    pub sequence_id: Option<i64>,
}

impl Message {
    pub fn is_thread_root(&self) -> bool {
        match (&self.id, &self.root_message_id) {
            (Some(id), Some(root)) => id == root,
            (Some(_), None) => true,
            _ => false,
        }
    }

    pub fn time_key(&self) -> &str {
        self.original_arrival_time.as_deref().or(self.compose_time.as_deref()).unwrap_or("")
    }
}

#[derive(Debug, Serialize)]
pub struct AuthStatus {
    pub user: Option<String>,
    pub name: Option<String>,
    pub tenant: Option<String>,
    pub region: String,
    pub chat_service: String,
    pub services: usize,
    pub tokens: Vec<TokenInfo>,
}

#[derive(Debug, Serialize)]
pub struct TokenInfo {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audience: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_in_min: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct MessageAction {
    pub action: &'static str,
    pub conversation: String,
    pub message_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub emoji: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Thread {
    pub root: Message,
    pub replies: Vec<Message>,
}

#[derive(Debug, Deserialize)]
pub struct CsaUpdates {
    #[serde(default)]
    pub teams: Vec<Team>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Team {
    pub id: String,
    #[serde(rename = "displayName", default)]
    pub display_name: String,
    #[serde(default)]
    pub channels: Vec<Channel>,
}

impl Team {
    pub fn matches(&self, query: &str) -> bool {
        let needle = query.to_lowercase();
        self.display_name.to_lowercase().contains(&needle) || self.id.to_lowercase().contains(&needle)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Channel {
    pub id: String,
    #[serde(rename = "displayName", default)]
    pub display_name: String,
    #[serde(rename = "isGeneral", default)]
    pub is_general: bool,
    #[serde(rename = "isFavorite", default)]
    pub is_favorite: bool,
}

#[derive(Debug, Deserialize)]
pub struct CalendarView {
    #[serde(default)]
    pub value: Vec<CalendarEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalendarEvent {
    #[serde(default)]
    pub subject: Option<String>,
    #[serde(default)]
    pub start: Option<DateTimeZone>,
    #[serde(default)]
    pub end: Option<DateTimeZone>,
    #[serde(default)]
    pub location: Option<Location>,
    #[serde(rename = "isAllDay", default)]
    pub is_all_day: bool,
    #[serde(rename = "isCancelled", default)]
    pub is_cancelled: bool,
    #[serde(rename = "isOnlineMeeting", default)]
    pub is_online_meeting: bool,
    #[serde(rename = "webLink", default)]
    pub web_link: Option<String>,
    #[serde(default)]
    pub organizer: Option<Organizer>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DateTimeZone {
    #[serde(rename = "dateTime", default)]
    pub date_time: Option<String>,
    #[serde(rename = "timeZone", default)]
    pub time_zone: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Location {
    #[serde(rename = "displayName", default)]
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Organizer {
    #[serde(rename = "emailAddress", default)]
    pub email_address: Option<EmailAddress>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailAddress {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub address: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AuthAction {
    pub action: &'static str,
    pub signed_in: bool,
}

#[derive(Debug, Deserialize)]
pub struct SubstrateSuggestions {
    #[serde(rename = "Groups", default)]
    pub groups: Vec<SuggestionGroup>,
}

impl SubstrateSuggestions {
    pub fn people(self) -> Vec<Person> {
        self.groups.into_iter().flat_map(|group| group.suggestions).collect()
    }
}

#[derive(Debug, Deserialize)]
pub struct SuggestionGroup {
    #[serde(rename = "Suggestions", default)]
    pub suggestions: Vec<Person>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Person {
    #[serde(rename = "Id", default)]
    pub id: Option<String>,
    #[serde(rename = "DisplayName", default)]
    pub display_name: Option<String>,
    #[serde(rename = "EmailAddresses", default)]
    pub email_addresses: Vec<String>,
    #[serde(rename = "JobTitle", default)]
    pub job_title: Option<String>,
    #[serde(rename = "Department", default)]
    pub department: Option<String>,
    #[serde(rename = "MRI", default)]
    pub mri: Option<String>,
}
