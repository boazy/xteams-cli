//! Deterministic candidate selection for `@{…}` mention queries — never
//! "first result": a mention notifies real people, so an unclear query is an
//! error naming the candidates (and their MRIs for the explicit form).

use crate::error::ContentError;
use crate::model::{Conversation, Person};

/// Choose the person a `@{query}` refers to. One candidate wins; among several,
/// a unique exact display-name or email match wins; anything else is an error.
pub fn pick_person(query: &str, people: &[Person]) -> Result<(String, String), ContentError> {
    let resolved = |p: &Person| {
        let name = p.display_name.clone().unwrap_or_else(|| query.to_owned());
        (p.mri.clone().unwrap_or_default(), name)
    };
    let candidates: Vec<&Person> = people.iter().filter(|p| p.mri.is_some()).collect();
    match candidates.as_slice() {
        [] => Err(ContentError::MentionNotFound(query.to_owned())),
        [one] => Ok(resolved(one)),
        many => {
            let q = query.to_lowercase();
            let exact: Vec<&&Person> = many
                .iter()
                .filter(|p| {
                    p.display_name
                        .as_deref()
                        .is_some_and(|n| n.to_lowercase() == q)
                        || p.email_addresses.iter().any(|e| e.to_lowercase() == q)
                })
                .collect();
            if let [one] = exact.as_slice() {
                return Ok(resolved(one));
            }
            let candidates = many
                .iter()
                .take(6)
                .map(|p| {
                    format!(
                        "{} <{}>",
                        p.display_name.as_deref().unwrap_or("?"),
                        p.mri.as_deref().unwrap_or("?")
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            Err(ContentError::MentionAmbiguous {
                query: query.to_owned(),
                candidates,
            })
        }
    }
}

/// Choose the channel a `@{#query}` refers to, from the chat-service
/// conversation list (the channels you follow — the same source as
/// `channel list`). The query must match a channel topic **exactly**
/// (case-insensitive) and uniquely — a substring never auto-selects a channel
/// (duplicate names across teams, e.g. "General", are common); near-misses are
/// reported as candidates.
pub fn pick_channel(
    query: &str,
    conversations: &[Conversation],
) -> Result<(String, String), ContentError> {
    let q = query.trim().to_lowercase();
    let mut exact = Vec::new();
    let mut near = Vec::new();
    for conv in conversations.iter().filter(|c| c.is_channel()) {
        let topic = conv.topic();
        if topic.to_lowercase() == q {
            exact.push(conv);
        } else if !topic.is_empty() && topic.to_lowercase().contains(&q) {
            near.push(conv);
        }
    }
    match exact.as_slice() {
        [one] => Ok((one.id.clone(), one.topic().to_owned())),
        [] if near.is_empty() => Err(ContentError::MentionNotFound(format!("#{query}"))),
        _ => {
            let listed = if exact.is_empty() { &near } else { &exact };
            let candidates = listed
                .iter()
                .take(6)
                .map(|c| format!("{} <{}>", c.topic(), c.id))
                .collect::<Vec<_>>()
                .join(", ");
            Err(ContentError::MentionAmbiguous {
                query: format!("#{query}"),
                candidates,
            })
        }
    }
}
#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::model::ThreadProperties;

    fn person(name: &str, mri: Option<&str>, emails: &[&str]) -> Person {
        Person {
            id: None,
            display_name: Some(name.to_owned()),
            email_addresses: emails.iter().map(|e| (*e).to_owned()).collect(),
            job_title: None,
            department: None,
            mri: mri.map(str::to_owned),
        }
    }

    fn channel(id: &str, topic: Option<&str>) -> Conversation {
        Conversation {
            id: id.to_owned(),
            thread_properties: Some(ThreadProperties {
                topic: topic.map(str::to_owned),
            }),
            last_message: None,
        }
    }

    fn hit(mri: &str, name: &str) -> (String, String) {
        (mri.to_owned(), name.to_owned())
    }

    #[test]
    fn person_single_candidate_wins() {
        let people = [person("Ada L", Some("8:orgid:1"), &[])];
        assert_eq!(
            pick_person("ada", &people).unwrap(),
            hit("8:orgid:1", "Ada L")
        );
    }

    #[test]
    fn person_prefers_unique_exact_name_or_email_match() {
        let people = [
            person("Ada Lovelace", Some("8:orgid:1"), &["ada@ex.com"]),
            person("Ada Lovelace-Smith", Some("8:orgid:2"), &["adals@ex.com"]),
        ];
        assert_eq!(
            pick_person("ada lovelace", &people).unwrap(),
            hit("8:orgid:1", "Ada Lovelace")
        );
        assert_eq!(
            pick_person("ADALS@EX.COM", &people).unwrap(),
            hit("8:orgid:2", "Ada Lovelace-Smith")
        );
    }

    #[test]
    fn person_rejects_none_and_ambiguous() {
        assert!(matches!(
            pick_person("q", &[]),
            Err(ContentError::MentionNotFound(_))
        ));
        let no_mri = [person("Ghost", None, &[])];
        assert!(matches!(
            pick_person("ghost", &no_mri),
            Err(ContentError::MentionNotFound(_))
        ));
        let two = [
            person("Ada A", Some("8:orgid:1"), &[]),
            person("Ada B", Some("8:orgid:2"), &[]),
        ];
        assert!(matches!(
            pick_person("ada", &two),
            Err(ContentError::MentionAmbiguous { .. })
        ));
    }

    #[test]
    fn channel_unique_exact_topic_wins_case_insensitively() {
        let convs = [
            channel("19:c1@thread.tacv2", Some("Proto Mapping")),
            channel("19:x@thread.v2", Some("proto mapping")), // group chat, not a channel
        ];
        assert_eq!(
            pick_channel("proto mapping", &convs).unwrap(),
            hit("19:c1@thread.tacv2", "Proto Mapping")
        );
    }

    #[test]
    fn channel_substring_never_auto_selects() {
        let convs = [channel("19:c1@thread.tacv2", Some("Proto Mapping"))];
        assert!(matches!(
            pick_channel("proto", &convs),
            Err(ContentError::MentionAmbiguous { .. })
        ));
    }

    #[test]
    fn channel_duplicate_topics_are_ambiguous() {
        let convs = [
            channel("19:c1@thread.tacv2", Some("General")),
            channel("19:c2@thread.tacv2", Some("General")),
        ];
        assert!(matches!(
            pick_channel("general", &convs),
            Err(ContentError::MentionAmbiguous { .. })
        ));
    }

    #[test]
    fn channel_unknown_or_topicless_is_not_found() {
        let convs = [
            channel("19:c1@thread.tacv2", Some("General")),
            channel("19:c2@thread.tacv2", None),
        ];
        assert!(matches!(
            pick_channel("nope", &convs),
            Err(ContentError::MentionNotFound(_))
        ));
    }
}
