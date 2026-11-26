//! Bind user-visible ticket confirmations to completed connector calls in this turn.
use crate::client_common::ResponseEvent;
use codex_extension_api::ExtensionData;
use codex_protocol::items::{McpToolCallItem, McpToolCallStatus};
use codex_protocol::models::{ContentItem, MessagePhase, ResponseItem};
use regex_lite::Regex;
use serde_json::Value;
use std::sync::{LazyLock, Mutex};

const UNCONFIRMED: &str =
    "I have no matching creation receipt and readback. I can’t confirm a ticket for this attempt.";
static KEY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b[A-Z][A-Z0-9]*-[1-9][0-9]*\b").unwrap());
static REPORT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:creat(?:e|ed|ion)|filed?|filing|open(?:ed)?|submit(?:ted)?|where.s my|where is (?:my|the))\b.{0,50}\b(?:ticket|issue)s?\b|\b(?:ticket|issue)(?: was| been| is| has been)? (?:creation|created|filed|opened|submitted)\b").unwrap()
});
static CLAIM: LazyLock<Regex> = LazyLock::new(|| {
    let action = r"(?:created|filed|opened|submitted|logged|raised)";
    let key = r"[a-z][a-z0-9]*-[1-9][0-9]*";
    let noun = r"(?:tickets?|issues?)";
    let article = r"(?:(?:a|an|the|your|new|linear|support)\s+)*";
    let auxiliary = r"(?:(?:has|have|now|been|is|was|were|successfully|just)\s+)*";
    let subject = format!(r"(?:{noun}(?:\s*\({key}\)|\s+{key})?|{key})");
    Regex::new(&format!(
        r"(?i)\b{action}\s+{article}(?:{noun}|{key})\b|\b{subject}\s+{auxiliary}{action}\b"
    ))
    .unwrap()
});

#[derive(PartialEq)]
struct Receipt {
    key: String,
    id: String,
    title: String,
    team: String,
    team_id: String,
}
impl Receipt {
    fn read(call: &McpToolCallItem) -> Option<Self> {
        let result = call.result.as_ref()?;
        if call.status != McpToolCallStatus::Completed
            || call.error.is_some()
            || result.is_error == Some(true)
        {
            return None;
        }
        let parsed;
        let row = if let Some(row) = &result.structured_content {
            row
        } else {
            let [part] = result.content.as_slice() else {
                return None;
            };
            if part["type"] != "text" {
                return None;
            }
            parsed = serde_json::from_str::<Value>(part["text"].as_str()?).ok()?;
            &parsed
        };
        if !row["error"].is_null() || row["success"] == false || row["isError"] == true {
            return None;
        }
        let key = row["identifier"].as_str().or(row["id"].as_str())?;
        if KEY.find(key)?.as_str() != key {
            return None;
        }
        let id = row["uuid"].as_str().or(row["id"].as_str())?;
        let title = row["title"].as_str()?;
        let team = row["team"]
            .as_str()
            .or(row["team"]["key"].as_str())
            .unwrap_or_default();
        let team_id = row["teamId"]
            .as_str()
            .or(row["team"]["id"].as_str())
            .unwrap_or_default();
        if id.is_empty() || title.trim().is_empty() || (team.is_empty() && team_id.is_empty()) {
            return None;
        }
        Some(Self {
            key: key.into(),
            id: id.into(),
            title: title.into(),
            team: team.into(),
            team_id: team_id.into(),
        })
    }
}

#[derive(PartialEq)]
struct Route {
    server: String,
    environment: Option<String>,
    account: Option<String>,
}
struct Attempt {
    route: Route,
    receipt: Option<Receipt>,
    confirmed: bool,
    failed: bool,
}
#[derive(Default)]
struct Attempts(Mutex<Vec<Attempt>>);

pub(crate) fn observe(turn: &ExtensionData, call: &McpToolCallItem) {
    let op = match call.server.as_str() {
        "linear" | "linear_direct" => call.tool.as_str(),
        "codex_apps" => match call.tool.as_str() {
            "linear_save_issue" => "save_issue",
            "linear_create_issue" | "linear.CreateIssue" => "create_issue",
            "linear_get_issue" | "linear.GetIssue" => "get_issue",
            _ => return,
        },
        _ => return,
    };
    if !matches!(op, "create_issue" | "save_issue" | "get_issue")
        || (op == "save_issue" && !call.arguments["id"].is_null())
    {
        return;
    }
    let requested = call.arguments["link_id"].as_str();
    let actual = call.link_id.as_deref();
    let identified = (requested.is_none() || actual.is_none() || requested == actual)
        && (call.server != "codex_apps" || actual.or(requested).is_some_and(|a| !a.is_empty()));
    let route = Route {
        server: call.server.clone(),
        environment: call.environment_id.clone(),
        account: actual.or(requested).map(str::to_owned),
    };
    let receipt = identified.then(|| Receipt::read(call)).flatten();
    let store = turn.get_or_init(Attempts::default);
    let mut attempts = store.0.lock().expect("ticket attempts mutex poisoned");
    if op == "get_issue" {
        if let Some(receipt) = receipt
            && call.arguments["id"]
                .as_str()
                .is_some_and(|id| id == receipt.key || id == receipt.id)
        {
            for a in attempts
                .iter_mut()
                .filter(|a| a.route == route && a.receipt.as_ref() == Some(&receipt))
            {
                a.confirmed = true;
            }
        }
        return;
    }
    let receipt = receipt.filter(|r| {
        call.arguments["title"].as_str() == Some(&r.title)
            && call.arguments["team"]
                .as_str()
                .is_some_and(|t| !t.is_empty() && (t == r.team || t == r.team_id))
    });
    attempts.push(Attempt {
        route,
        receipt,
        confirmed: false,
        failed: call.status == McpToolCallStatus::Failed,
    });
}

fn live_report(turn: &ExtensionData) -> Option<String> {
    let store = turn.get::<Attempts>()?;
    let attempts = store.0.lock().expect("ticket attempts mutex poisoned");
    if attempts.is_empty() {
        return None;
    }
    Some(attempts.iter().enumerate().map(|(i,a)|{
        let text=match (&a.receipt,a.confirmed,a.failed) {
            (Some(r),true,_)=>format!("Created {}. A separate lookup confirmed its title and team.",r.key),
            (Some(r),false,_)=>format!("The write returned {}. I have no matching readback yet.",r.key),
            (_,_,true)=>"The write failed. I can’t confirm a ticket for this attempt. The cause is unknown.".into(),
            _=>UNCONFIRMED.into(),
        };
        if attempts.len()==1 {text} else {format!("Attempt {}. {text}",i+1)}
    }).collect::<Vec<_>>().join("\n"))
}

// Reports about quoted incidents are attributed to the writer's record. They
// never populate the live receipt ledger. A lookup alone cannot prove a write.
fn incident_report(prompt: &str) -> Option<String> {
    static LABEL: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?:^|\s)([A-Z]|[1-9][0-9]*)\.\s+").unwrap());
    static WRITE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)\b(?:retry|write|creation|create|filing)\b[^.!?]*\breturned\s+([A-Z][A-Z0-9]*-[1-9][0-9]*)\b").unwrap()
    });
    let labels: Vec<_> = LABEL.captures_iter(prompt).collect();
    let sections: Vec<_> = if labels.is_empty() {
        vec![("", prompt)]
    } else {
        labels
            .iter()
            .enumerate()
            .map(|(i, m)| {
                let start = m.get(0).unwrap().end();
                let end = labels
                    .get(i + 1)
                    .map_or(prompt.len(), |n| n.get(0).unwrap().start());
                (m.get(1).unwrap().as_str(), &prompt[start..end])
            })
            .collect()
    };
    let mut replies = Vec::new();
    for (label, section) in sections {
        let statements: Vec<_> = section
            .split_terminator(['.', '?', '!', '\n'])
            .map(str::trim)
            .collect();
        let observation = statements.iter().find(|s| {
            let t = s.to_ascii_lowercase();
            (t.contains("returned") || t.contains("failed"))
                && ["write", "creat", "ticket", "issue", "retry"]
                    .iter()
                    .any(|w| t.contains(w))
        });
        let Some(observation) = observation else {
            continue;
        };
        let keys: Vec<_> = KEY.find_iter(section).map(|m| m.as_str()).collect();
        let read = statements.iter().find(|s| {
            let t = s.to_ascii_lowercase();
            (t.starts_with("a lookup confirmed ")
                || t.starts_with("the readback confirmed ")
                || t.starts_with("a readback confirmed ")
                || t.starts_with("the lookup confirmed "))
                && t.contains("title")
                && (t.contains("workspace") || t.contains("team"))
                && !["not ", "wrong", "except", "only", "mismatch", "but "]
                    .iter()
                    .any(|w| t.contains(w))
        });
        let mut text = if label.is_empty() {
            String::new()
        } else {
            format!("{label}. ")
        };
        text.push_str(&format!("The record says “{observation}.” "));
        if let (Some(key), Some(lookup)) =
            (WRITE.captures(observation).and_then(|c| c.get(1)), read)
            && keys.iter().all(|k| *k == key.as_str())
            && !observation.to_ascii_lowercase().contains("not ")
        {
            text.push_str(&format!(
                "“{lookup}.” The record confirms {} for this attempt.",
                key.as_str()
            ));
        } else {
            text.push_str("I can’t confirm a ticket for this attempt.");
        }
        text.push_str(" The cause of the earlier failure is unknown.");
        if section.to_ascii_lowercase().contains("stale") {
            text.push_str(" The record does not establish that the chat was stale.");
        }
        if ["restart", "relaunch", "reboot"]
            .iter()
            .any(|w| section.to_ascii_lowercase().contains(w))
        {
            text.push_str(" The record does not establish whether restarting fixes it.");
        }
        replies.push(text);
    }
    (!replies.is_empty()).then(|| replies.join("\n\n"))
}

pub(crate) fn for_request(turn: &ExtensionData, input: &[ResponseItem]) -> Option<String> {
    if let Some(reply) = live_report(turn) {
        return Some(reply);
    }
    let text = input.iter().rev().find_map(|i| match i {
        ResponseItem::Message { role, content, .. } if role == "user" => Some(
            content
                .iter()
                .filter_map(|c| {
                    if let ContentItem::InputText { text } = c {
                        Some(text.as_str())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        _ => None,
    })?;
    let follow_up = [
        "did you create it",
        "did it work",
        "was it created",
        "did that work",
        "where is it",
    ]
    .iter()
    .any(|p| text.trim_start().to_ascii_lowercase().starts_with(p));
    if !REPORT.is_match(&text) {
        if follow_up && input.iter().rev().filter(|i|matches!(i,ResponseItem::Message{role,..} if role=="user")).skip(1).take(1).any(|i|matches!(i,ResponseItem::Message{content,..} if content.iter().any(|p|matches!(p,ContentItem::InputText{text} if REPORT.is_match(text))))) {
            return Some(UNCONFIRMED.into());
        }
        return None;
    }
    let lower = text.to_ascii_lowercase();
    if lower.contains("fictional") || lower.contains("hypothetical") {
        return incident_report(&text).or_else(|| Some(UNCONFIRMED.into()));
    }
    // Keep implementation/explanation requests on the normal model path.
    if [
        "implement ",
        "explain ",
        "fix ",
        "test ",
        "debug ",
        "document ",
        "how to ",
        "how do ",
    ]
    .iter()
    .any(|p| lower.trim_start().starts_with(p))
    {
        return None;
    }
    Some(UNCONFIRMED.into())
}

// Keep the provider's opaque continuation item. Its optional display text is
// another client-visible channel and cannot attest an operation.
pub(crate) fn filter_reasoning(event: &mut ResponseEvent) -> bool {
    match event {
        ResponseEvent::OutputItemAdded(ResponseItem::Reasoning {
            summary, content, ..
        })
        | ResponseEvent::OutputItemDone(ResponseItem::Reasoning {
            summary, content, ..
        }) => {
            summary.clear();
            *content = None;
            true
        }
        ResponseEvent::ReasoningContentDelta { .. }
        | ResponseEvent::ReasoningSummaryDelta { .. }
        | ResponseEvent::ReasoningSummaryPartAdded { .. }
        | ResponseEvent::ReasoningSummaryDone { .. } => false,
        _ => true,
    }
}

#[derive(Default)]
pub(crate) struct MessageStream {
    report: Option<String>,
    pending: String,
    released: usize,
    active: bool,
    held: bool,
}

// Keep ordinary prose flowing. Once a potential ticket assertion starts,
// hold its suffix until the completed item can be checked, including split tokens.
fn possible_claim(text: &str) -> Option<(usize, bool)> {
    const WORDS: &[&str] = &[
        "ticket",
        "issue",
        "created",
        "filed",
        "opened",
        "raised",
        "submitted",
        "logged",
    ];
    let mut words = text.char_indices().peekable();
    while let Some((start, c)) = words.next() {
        if !c.is_ascii_alphanumeric() {
            continue;
        }
        let mut end = start + c.len_utf8();
        while let Some(&(i, next)) = words.peek() {
            if !next.is_ascii_alphanumeric() && next != '-' {
                break;
            }
            words.next();
            end = i + next.len_utf8();
        }
        let token = &text[start..end];
        let lower = token.to_ascii_lowercase();
        let key_candidate = token.as_bytes()[0].is_ascii_uppercase()
            && token
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'-');
        if WORDS
            .iter()
            .any(|w| lower == *w || lower.strip_suffix('s') == Some(*w))
            || (key_candidate && token.contains('-'))
        {
            return Some((start, true));
        }
        if end == text.len() && (WORDS.iter().any(|w| w.starts_with(&lower)) || key_candidate) {
            return Some((start, false));
        }
    }
    None
}

impl MessageStream {
    pub(crate) fn start(&mut self, item: &mut ResponseItem, report: Option<&str>) {
        *self = Self::default();
        let ResponseItem::Message {
            role,
            content,
            phase,
            ..
        } = item
        else {
            return;
        };
        if role != "assistant" {
            return;
        }
        self.active = true;
        self.report = report.map(|s| {
            if *phase == Some(MessagePhase::Commentary) {
                "Checking the ticket record.".into()
            } else {
                s.to_owned()
            }
        });
        for part in content.iter_mut() {
            if let ContentItem::OutputText {
                text,
                annotations,
                logprobs,
            } = part
            {
                let accepted = self.push(std::mem::take(text));
                *text = accepted;
                if self.report.is_some() || !self.pending.is_empty() {
                    *annotations = None;
                    *logprobs = None;
                }
            }
        }
    }

    pub(crate) fn push(&mut self, delta: String) -> String {
        if !self.active {
            return delta;
        }
        if self.report.is_some() {
            return String::new();
        }
        if self.held {
            return String::new();
        }
        self.pending.push_str(&delta);
        let available = match possible_claim(&self.pending) {
            Some((start, whole)) => {
                self.held = whole;
                start
            }
            None => self.pending.len(),
        };
        let tail = self.pending.split_off(available);
        let accepted = std::mem::replace(&mut self.pending, tail);
        self.released += accepted.len();
        accepted
    }

    pub(crate) fn finish(
        &mut self,
        item: &mut ResponseItem,
        report: Option<&str>,
    ) -> Option<String> {
        let ResponseItem::Message {
            role,
            content,
            phase,
            encrypted_content,
            ..
        } = item
        else {
            return None;
        };
        if role != "assistant" {
            return None;
        }
        let full = content
            .iter()
            .filter_map(|p| match p {
                ContentItem::OutputText { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect::<String>();
        if full.is_empty() {
            return None;
        }
        let scoped = self.report.as_deref().or(report);
        let prefix = full.get(..self.released).unwrap_or_default();
        let replacement = if let Some(s) = scoped {
            Some(if *phase == Some(MessagePhase::Commentary) {
                "Checking the ticket record.".to_owned()
            } else {
                s.to_owned()
            })
        } else if CLAIM.is_match(&full) {
            Some(if prefix.trim().is_empty() {
                UNCONFIRMED.into()
            } else if prefix.ends_with("I ") || prefix.ends_with("We ") {
                format!("{prefix}cannot confirm that a ticket was created.")
            } else if prefix.ends_with("Your ") || prefix.ends_with("The ") {
                format!("{prefix}ticket creation is unconfirmed.")
            } else {
                format!("{prefix}[ticket creation unconfirmed].")
            })
        } else {
            None
        };
        let suffix = if let Some(s) = replacement {
            let delta = s.strip_prefix(prefix).unwrap_or(&s).to_owned();
            *content = vec![ContentItem::OutputText {
                text: s,
                annotations: None,
                logprobs: None,
            }];
            *encrypted_content = None;
            Some(delta)
        } else if self.held || !self.pending.is_empty() {
            Some(full.get(self.released..).unwrap_or_default().to_owned())
        } else {
            None
        };
        *self = Self::default();
        suffix.filter(|s| !s.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn message(role: &str, text: &str) -> ResponseItem {
        let kind = if role == "user" {
            "input_text"
        } else {
            "output_text"
        };
        serde_json::from_value(
            json!({"type":"message","role":role,"content":[{"type":kind,"text":text}]}),
        )
        .unwrap()
    }

    fn mcp(op: &str, id: &str, args: Value, row: Value) -> McpToolCallItem {
        serde_json::from_value(json!({"id":id,"server":"codex_apps","tool":op,"arguments":args,
            "linkId":"account-a","status":"completed","result":{"content":[],"structuredContent":row}})).unwrap()
    }

    #[test]
    fn confirmation_requires_matching_write_and_later_read_in_the_same_account() {
        let turn = ExtensionData::new("turn");
        let row = json!({"id":"OPS-31","uuid":"issue-uuid","title":"Printer","team":"Support","teamId":"team-id"});
        let write = mcp(
            "linear_save_issue",
            "w",
            json!({"title":"Printer","team":"team-id","link_id":"account-a"}),
            row.clone(),
        );
        let lookup = mcp(
            "linear_get_issue",
            "r",
            json!({"id":"OPS-31","link_id":"account-a"}),
            row.clone(),
        );
        observe(&turn, &lookup);
        assert!(live_report(&turn).is_none());
        let mut failed = write.clone();
        failed.status = McpToolCallStatus::Failed;
        observe(&turn, &failed);
        observe(&turn, &write);
        for changed in [
            json!({"teamId":"other"}),
            json!({"uuid":"another-issue"}),
            json!({"title":"Other title"}),
        ] {
            let mut wrong = row.clone();
            wrong
                .as_object_mut()
                .unwrap()
                .extend(changed.as_object().unwrap().clone());
            observe(
                &turn,
                &mcp("linear_get_issue", "wrong", lookup.arguments.clone(), wrong),
            );
        }
        let mut other = lookup.clone();
        other.link_id = Some("other-account".into());
        observe(&turn, &other);
        assert!(!live_report(&turn).unwrap().contains("Created"));
        observe(&turn, &lookup);
        let answer = live_report(&turn).unwrap();
        assert!(!answer.lines().next().unwrap().contains("OPS-31"));
        assert!(answer.lines().nth(1).unwrap().contains("Created OPS-31."));
        assert!(live_report(&ExtensionData::new("other turn")).is_none());
        let only_update = ExtensionData::new("update");
        let mut update = write;
        update.arguments["id"] = json!("OPS-31");
        observe(&only_update, &update);
        observe(&only_update, &lookup);
        assert!(live_report(&only_update).is_none());
    }

    #[test]
    fn fictional_records_keep_attempts_separate_and_never_supply_live_receipts() {
        let turn = ExtensionData::new("fiction");
        let prompt = "Report the fictional ticket creation records. A. The create call failed. Is the chat stale? B. The retry returned OPS-91. A readback confirmed the title and team.";
        let answer = for_request(&turn, &[message("user", prompt)]).unwrap();
        let (a, b) = answer.split_once("\n\n").unwrap();
        assert!(!a.contains("OPS-91"));
        assert!(a.contains("can’t confirm"));
        assert!(b.contains("record confirms OPS-91"));
        assert!(
            for_request(
                &turn,
                &[message(
                    "user",
                    "Please file a ticket. The retry returned OPS-91."
                )]
            )
            .unwrap()
            .contains("no matching")
        );
        assert!(live_report(&turn).is_none());
    }

    #[test]
    fn guarded_reasoning_keeps_continuation_but_does_not_publish_a_claim() {
        let item = serde_json::from_value(
            json!({"type":"reasoning","id":"r","encrypted_content":"opaque",
            "summary":[{"type":"summary_text","text":"Created OPS-12"}]}),
        )
        .unwrap();
        let mut event = ResponseEvent::OutputItemDone(item);
        assert!(filter_reasoning(&mut event));
        assert!(
            matches!(event,ResponseEvent::OutputItemDone(ResponseItem::Reasoning{
            summary,content:None,encrypted_content:Some(value),..}) if summary.is_empty() && value=="opaque")
        );
        assert!(!filter_reasoning(
            &mut ResponseEvent::ReasoningSummaryDelta {
                delta: "Created OPS-12".into(),
                summary_index: 0
            }
        ));
    }

    #[test]
    fn sanitized_delta_and_saved_item_agree_even_on_seeded_or_split_provider_text() {
        let mut stream = MessageStream::default();
        let mut start = message("assistant", "I created ");
        stream.start(&mut start, Some(UNCONFIRMED));
        assert_eq!(
            crate::stream_events_utils::raw_assistant_output_text_from_item(&start).as_deref(),
            Some("")
        );
        assert_eq!(stream.push("a ticket!".into()), "");
        let mut done = message("assistant", "I created a ticket!");
        assert_eq!(
            stream.finish(&mut done, Some(UNCONFIRMED)).as_deref(),
            Some(UNCONFIRMED)
        );
        assert_eq!(
            crate::stream_events_utils::raw_assistant_output_text_from_item(&done).as_deref(),
            Some(UNCONFIRMED)
        );
        for raw in [
            "I created OPS-12.",
            "Your ticket (OPS-12) was created successfully.",
            "The file opened normally.",
            "Today looks fine.",
        ] {
            let mut initial = message("assistant", "");
            stream.start(&mut initial, None);
            let mut shown = String::new();
            for letter in raw.chars() {
                shown += &stream.push(letter.to_string())
            }
            let mut final_item = message("assistant", raw);
            shown += &stream.finish(&mut final_item, None).unwrap_or_default();
            assert_eq!(
                Some(&shown),
                crate::stream_events_utils::raw_assistant_output_text_from_item(&final_item)
                    .as_ref()
            );
            if raw.contains("OPS-12") {
                assert!(!shown.contains("OPS-12"));
            } else {
                assert_eq!(shown, raw);
            }
        }
    }
}
