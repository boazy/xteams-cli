//! Parsing Teams "deep links" — the `/l/<type>/…` URLs the New Teams desktop and
//! web apps generate — into the fields our commands care about: a conversation id
//! and, optionally, a message / parent-message id.
//!
//! We do **not** validate the id format: ids are opaque strings handed straight to
//! Teams (see AGENTS.md). The struct is deliberately a flat bag of optionals so a
//! caller takes only what it needs, regardless of the link kind. A future
//! `TryFrom<TeamsDeepLinkFields>` can build a fully-typed link enum.

/// Which `/l/<type>/` deep link this is. Informational — callers read the
/// extracted fields directly and don't branch on the kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TeamsDeepLinkKind {
    Message,
    Channel,
    Chat,
    Team,
    MeetupJoin,
    Entity,
    Call,
    Meeting,
    App,
    File,
    Task,
    MeetingShare,
    Unknown,
}

impl TeamsDeepLinkKind {
    fn parse(segment: &str) -> Self {
        match segment {
            "message" => Self::Message,
            "channel" => Self::Channel,
            "chat" => Self::Chat,
            "team" => Self::Team,
            "meetup-join" => Self::MeetupJoin,
            "entity" => Self::Entity,
            "call" => Self::Call,
            "meeting" => Self::Meeting,
            "app" => Self::App,
            "file" => Self::File,
            "task" => Self::Task,
            "meeting-share" => Self::MeetingShare,
            _ => Self::Unknown,
        }
    }

    /// Whether the conversation id sits in the first path segment after the type
    /// (`/l/<type>/<conversationId>/…`).
    fn carries_conversation(self) -> bool {
        matches!(
            self,
            Self::Message | Self::Channel | Self::Chat | Self::Team | Self::MeetupJoin
        )
    }
}

/// Everything we can pull out of a deep link. Fields are optional because
/// different link kinds carry different data.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)] // metadata fields are parsed for completeness / future typed-link conversion
pub struct TeamsDeepLinkFields {
    pub kind: TeamsDeepLinkKind,
    pub conversation_id: Option<String>,
    /// Message id from the URL path (`…/message/<conv>/<messageId>`).
    pub message_id: Option<String>,
    /// `parentMessageId` query param — the thread root of a reply.
    pub parent_message_id: Option<String>,
    pub tenant_id: Option<String>,
    pub group_id: Option<String>,
    pub channel_name: Option<String>,
    pub team_name: Option<String>,
}

impl TeamsDeepLinkFields {
    /// Id to use when the target is one specific message: the path message id.
    pub fn message_ref(&self) -> Option<&str> {
        self.message_id.as_deref()
    }

    /// Id to use when the target is a thread root: prefer `parentMessageId`,
    /// otherwise fall back to the path message id.
    pub fn thread_ref(&self) -> Option<&str> {
        self.parent_message_id
            .as_deref()
            .or(self.message_id.as_deref())
    }
}

const HOSTS: [&str; 3] = [
    "teams.microsoft.com",
    "teams.cloud.microsoft",
    "teams.live.com",
];

/// Parse a Teams deep link. Returns `None` when `input` is not a Teams `/l/…`
/// link (e.g. a raw conversation id), so callers fall back to treating the
/// argument as a literal id.
pub fn extract_teams_link_data(input: &str) -> Option<TeamsDeepLinkFields> {
    let input = input.trim();
    let looks_like_teams =
        input.starts_with("msteams:") || HOSTS.iter().any(|host| input.contains(host));
    if !looks_like_teams {
        return None;
    }

    // Everything from the `/l/` deep-link marker onward: `/l/<type>/<…>?<query>`.
    let rest = input.get(input.find("/l/")?..)?;
    let (path, query) = match rest.split_once('?') {
        Some((path, query)) => (path, Some(query)),
        None => (rest, None),
    };

    // segments[0] == "l", segments[1] == <type>, segments[2..] == data.
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let kind = TeamsDeepLinkKind::parse(segments.get(1)?);

    let conversation_id = if kind.carries_conversation() {
        segments.get(2).map(|s| decode(s)).filter(|id| id != "0")
    } else {
        None
    };
    let message_id = match kind {
        TeamsDeepLinkKind::Message => segments.get(3).map(|s| decode(s)),
        _ => None,
    };

    let mut fields = TeamsDeepLinkFields {
        kind,
        conversation_id,
        message_id,
        parent_message_id: None,
        tenant_id: None,
        group_id: None,
        channel_name: None,
        team_name: None,
    };
    if let Some(query) = query {
        for (key, value) in query.split('&').filter_map(|pair| pair.split_once('=')) {
            let value = decode(value);
            match key {
                "parentMessageId" => fields.parent_message_id = Some(value),
                "tenantId" => fields.tenant_id = Some(value),
                "groupId" => fields.group_id = Some(value),
                "channelName" => fields.channel_name = Some(value),
                "teamName" => fields.team_name = Some(value),
                _ => {}
            }
        }
    }
    Some(fields)
}

/// Resolve a `conversation` CLI argument that may be a Teams deep link. Returns
/// the conversation id (from the link, or the argument verbatim) and the parsed
/// link fields when the argument was a recognised link.
pub fn resolve_conversation(arg: String) -> (String, Option<TeamsDeepLinkFields>) {
    match extract_teams_link_data(&arg) {
        Some(fields) => {
            let conversation = fields.conversation_id.clone().unwrap_or(arg);
            (conversation, Some(fields))
        }
        None => (arg, None),
    }
}

fn decode(raw: &str) -> String {
    urlencoding::decode(raw)
        .map(|cow| cow.into_owned())
        .unwrap_or_else(|_| raw.to_owned())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn channel_link_percent_encoded() {
        let url = "https://teams.microsoft.com/l/channel/19%3A3SowqPWva8jvnD7ub4v_oAq-Cawno4p66eWXJ9_IXzo1%40thread.tacv2/General?groupId=b7dc1df1-ba01-40af-b54c-fb4b8a43c8d4&tenantId=af559f8b-54dc-49fe-8006-3cc5f2201ef3";
        let fields = extract_teams_link_data(url).unwrap();
        assert_eq!(fields.kind, TeamsDeepLinkKind::Channel);
        assert_eq!(
            fields.conversation_id.as_deref(),
            Some("19:3SowqPWva8jvnD7ub4v_oAq-Cawno4p66eWXJ9_IXzo1@thread.tacv2")
        );
        assert_eq!(fields.message_id, None);
        assert_eq!(
            fields.group_id.as_deref(),
            Some("b7dc1df1-ba01-40af-b54c-fb4b8a43c8d4")
        );
        assert_eq!(
            fields.tenant_id.as_deref(),
            Some("af559f8b-54dc-49fe-8006-3cc5f2201ef3")
        );
    }

    #[test]
    fn message_link_channel_literal_ids() {
        let url = "https://teams.microsoft.com/l/message/19:3SowqPWva8jvnD7ub4v_oAq-Cawno4p66eWXJ9_IXzo1@thread.tacv2/1783347413274?tenantId=af559f8b-54dc-49fe-8006-3cc5f2201ef3&groupId=b7dc1df1-ba01-40af-b54c-fb4b8a43c8d4&parentMessageId=1783326624650&teamName=Engineering&channelName=General&createdTime=1783347413274";
        let fields = extract_teams_link_data(url).unwrap();
        assert_eq!(fields.kind, TeamsDeepLinkKind::Message);
        assert_eq!(
            fields.conversation_id.as_deref(),
            Some("19:3SowqPWva8jvnD7ub4v_oAq-Cawno4p66eWXJ9_IXzo1@thread.tacv2")
        );
        assert_eq!(fields.message_id.as_deref(), Some("1783347413274"));
        assert_eq!(fields.parent_message_id.as_deref(), Some("1783326624650"));
        assert_eq!(fields.channel_name.as_deref(), Some("General"));
        assert_eq!(fields.team_name.as_deref(), Some("Engineering"));
        // A specific message uses the path id; a thread prefers the parent id.
        assert_eq!(fields.message_ref(), Some("1783347413274"));
        assert_eq!(fields.thread_ref(), Some("1783326624650"));
    }

    #[test]
    fn message_link_group_chat_cloud_host() {
        let url = "https://teams.cloud.microsoft/l/message/19:5d06c4fd8b1f42cf9a3ac3e5cac65401@thread.v2/1783444859454?context=%7B%22contextType%22%3A%22chat%22%7D";
        let fields = extract_teams_link_data(url).unwrap();
        assert_eq!(fields.kind, TeamsDeepLinkKind::Message);
        assert_eq!(
            fields.conversation_id.as_deref(),
            Some("19:5d06c4fd8b1f42cf9a3ac3e5cac65401@thread.v2")
        );
        assert_eq!(fields.message_id.as_deref(), Some("1783444859454"));
        assert_eq!(fields.parent_message_id, None);
        // No parent → a thread reference falls back to the path id.
        assert_eq!(fields.thread_ref(), Some("1783444859454"));
    }

    #[test]
    fn message_link_cloud_host_with_parent() {
        let url = "https://teams.cloud.microsoft/l/message/19:eb54c79164524febbdbdf0196449971d@thread.tacv2/1783446868678?tenantId=af559f8b-54dc-49fe-8006-3cc5f2201ef3&groupId=b7dc1df1-ba01-40af-b54c-fb4b8a43c8d4&parentMessageId=1783418929513&teamName=Engineering&channelName=Indigo%20MXDR&createdTime=1783446868678";
        let fields = extract_teams_link_data(url).unwrap();
        assert_eq!(
            fields.conversation_id.as_deref(),
            Some("19:eb54c79164524febbdbdf0196449971d@thread.tacv2")
        );
        assert_eq!(fields.message_id.as_deref(), Some("1783446868678"));
        assert_eq!(fields.parent_message_id.as_deref(), Some("1783418929513"));
        assert_eq!(fields.channel_name.as_deref(), Some("Indigo MXDR"));
    }

    #[test]
    fn raw_ids_are_not_links() {
        assert!(extract_teams_link_data("19:abc123@thread.tacv2").is_none());
        assert!(extract_teams_link_data("48:notes").is_none());
        assert!(
            extract_teams_link_data("19:a_b@unq.gbl.spaces").is_none(),
            "a raw 1:1 chat id is not a link"
        );
    }

    #[test]
    fn chat_link_with_conversations_suffix() {
        let url = "https://teams.microsoft.com/l/chat/19:c6d70e392a384916c3262b15406d763e@thread.v2/conversations";
        let fields = extract_teams_link_data(url).unwrap();
        assert_eq!(fields.kind, TeamsDeepLinkKind::Chat);
        assert_eq!(
            fields.conversation_id.as_deref(),
            Some("19:c6d70e392a384916c3262b15406d763e@thread.v2")
        );
    }

    #[test]
    fn new_chat_link_has_no_conversation() {
        let url = "https://teams.microsoft.com/l/chat/0/0?users=joe@contoso.com&topicName=Hi";
        let fields = extract_teams_link_data(url).unwrap();
        assert_eq!(fields.kind, TeamsDeepLinkKind::Chat);
        assert_eq!(fields.conversation_id, None);
    }

    #[test]
    fn msteams_protocol_v1() {
        let url = "msteams:/l/message/19:abc@thread.tacv2/123?parentMessageId=99";
        let fields = extract_teams_link_data(url).unwrap();
        assert_eq!(
            fields.conversation_id.as_deref(),
            Some("19:abc@thread.tacv2")
        );
        assert_eq!(fields.message_id.as_deref(), Some("123"));
        assert_eq!(fields.thread_ref(), Some("99"));
    }

    #[test]
    fn resolve_conversation_passes_raw_through() {
        let (id, link) = resolve_conversation("19:abc@thread.tacv2".to_owned());
        assert_eq!(id, "19:abc@thread.tacv2");
        assert!(link.is_none());
    }

    #[test]
    fn resolve_conversation_extracts_from_link() {
        let (id, link) = resolve_conversation(
            "https://teams.microsoft.com/l/message/19:abc@thread.tacv2/123".to_owned(),
        );
        assert_eq!(id, "19:abc@thread.tacv2");
        assert_eq!(link.unwrap().message_id.as_deref(), Some("123"));
    }
}
