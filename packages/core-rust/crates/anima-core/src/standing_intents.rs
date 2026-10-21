//! Bounded deterministic conversational intentions. The host proves owner provenance
//! and saves this candidate together with admission before any model dispatch.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
pub const MAX_INTENTS: usize = 64;
pub const MAX_CHANGES: usize = 256;
pub const MAX_ADMISSIONS: usize = 2048;
pub const MAX_INSTRUCTION_BYTES: usize = 2048;
pub const MAX_FIRES: u32 = 32;
pub const MAX_LIFETIME_MS: u64 = 366 * 24 * 60 * 60 * 1000;
pub const MAX_TURN_BYTES: usize = 65536;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SenderScope {
    Owner,
    LocalOwner,
    TelegramOwner,
    DiscordOwner,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OwnerSender {
    Local,
    Telegram,
    Discord,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IntentScope {
    pub session_id: Option<String>,
    pub sender: SenderScope,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationEvent {
    #[serde(rename = "conversation.started")]
    Started,
    #[serde(rename = "conversation.message")]
    Message,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum IntentCondition {
    Keyword { phrase: String },
    Event { event: ConversationEvent },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IntentSpec {
    pub scope: IntentScope,
    pub condition: IntentCondition,
    pub instruction: String,
    pub expires_at_ms: u64,
    pub cooldown_ms: u64,
    pub max_fires: u32,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IntentFire {
    pub run_id: String,
    pub at_ms: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StandingIntent {
    pub id: String,
    pub agent_id: String,
    pub spec: IntentSpec,
    pub revision: u64,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub cancelled: bool,
    pub fires: Vec<IntentFire>,
}
impl StandingIntent {
    pub fn instruction(&self) -> &str {
        &self.spec.instruction
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IntentChange {
    pub id: String,
    pub before: Option<StandingIntent>,
    pub after: StandingIntent,
    pub undone: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IntentAdmission {
    pub agent_id: String,
    pub run_id: String,
    pub retry_key: Option<String>,
    pub intent_ids: Vec<String>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StandingIntentRegistry {
    pub intents: BTreeMap<String, StandingIntent>,
    pub changes: BTreeMap<String, IntentChange>,
    pub admissions: Vec<IntentAdmission>,
}
pub struct OwnerTurn<'a> {
    pub agent_id: &'a str,
    pub session_id: &'a str,
    pub sender: OwnerSender,
    pub run_id: &'a str,
    pub retry_key: Option<&'a str>,
    pub text: &'a str,
    pub conversation_started: bool,
    pub now_ms: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntentMatch {
    pub intent_id: String,
    pub instruction: String,
}
fn bounded_id(s: &str) -> bool {
    !s.trim().is_empty() && s.len() <= 256 && !s.chars().any(char::is_control)
}
fn spec_valid(s: &IntentSpec) -> bool {
    s.scope.session_id.as_deref().is_none_or(bounded_id)
        && !s.instruction.trim().is_empty()
        && s.instruction.len() <= MAX_INSTRUCTION_BYTES
        && s.max_fires > 0
        && s.max_fires <= MAX_FIRES
        && s.cooldown_ms <= MAX_LIFETIME_MS
        && match &s.condition {
            IntentCondition::Keyword { phrase } => {
                !phrase.trim().is_empty()
                    && phrase.len() <= 128
                    && !phrase.chars().any(char::is_control)
            }
            IntentCondition::Event { .. } => true,
        }
}
fn record_valid(r: &StandingIntent) -> bool {
    bounded_id(&r.id)
        && bounded_id(&r.agent_id)
        && spec_valid(&r.spec)
        && r.revision > 0
        && r.created_at_ms <= r.updated_at_ms
        && r.spec.expires_at_ms > r.created_at_ms
        && r.spec.expires_at_ms <= r.updated_at_ms.saturating_add(MAX_LIFETIME_MS)
        && r.fires.len() <= r.spec.max_fires as usize
        && r.fires.iter().all(|f| {
            bounded_id(&f.run_id) && f.at_ms >= r.created_at_ms && f.at_ms < r.spec.expires_at_ms
        })
        && r.fires.windows(2).all(|p| p[0].at_ms <= p[1].at_ms)
        && r.fires
            .iter()
            .map(|f| &f.run_id)
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            == r.fires.len()
}
fn normalize(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}
fn phrase_matches(text: &str, phrase: &str) -> bool {
    let text = normalize(text);
    let phrase = normalize(phrase);
    text.match_indices(&phrase).any(|(at, _)| {
        text[..at]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_alphanumeric())
            && text[at + phrase.len()..]
                .chars()
                .next()
                .is_none_or(|c| !c.is_alphanumeric())
    })
}
impl StandingIntentRegistry {
    pub fn validate(&self) -> Result<(), String> {
        if self.intents.len() > MAX_INTENTS
            || self.changes.len() > MAX_CHANGES
            || self.admissions.len() > MAX_ADMISSIONS
        {
            return Err("Standing intention history exceeds its bound".into());
        }
        if self
            .intents
            .iter()
            .any(|(id, r)| id != &r.id || !record_valid(r))
        {
            return Err("Invalid standing intention".into());
        }
        if self.changes.iter().any(|(id, c)| {
            id != &c.id
                || !bounded_id(id)
                || !record_valid(&c.after)
                || c.before.as_ref().is_some_and(|b| {
                    !record_valid(b) || b.id != c.after.id || b.agent_id != c.after.agent_id
                })
        }) {
            return Err("Invalid standing intention change image".into());
        }
        let mut runs = std::collections::BTreeSet::new();
        let mut keys = std::collections::BTreeSet::new();
        for a in &self.admissions {
            if !bounded_id(&a.agent_id)
                || !bounded_id(&a.run_id)
                || a.retry_key.as_deref().is_some_and(|k| !bounded_id(k))
                || a.intent_ids.is_empty()
                || a.intent_ids.len() > 3
                || a.intent_ids.iter().any(|id| !bounded_id(id))
                || a.intent_ids
                    .iter()
                    .collect::<std::collections::BTreeSet<_>>()
                    .len()
                    != a.intent_ids.len()
                || !runs.insert((&a.agent_id, &a.run_id))
                || a.retry_key
                    .as_ref()
                    .is_some_and(|k| !keys.insert((&a.agent_id, k)))
            {
                return Err("Invalid standing intention admission".into());
            }
        }
        Ok(())
    }
    fn commit_change(
        &mut self,
        before: Option<StandingIntent>,
        after: StandingIntent,
    ) -> Result<(StandingIntent, IntentChange), String> {
        if !record_valid(&after) {
            return Err("Standing intention change history is full or invalid".into());
        }
        if self.changes.len() >= MAX_CHANGES {
            // Reclaim only handles that already refuse Undo. Monotonic revisions
            // prevent these old after-images from becoming current again.
            self.changes.retain(|_, change| {
                !change.undone && self.intents.get(&change.after.id) == Some(&change.after)
            });
            if self.changes.len() >= MAX_CHANGES {
                return Err("Standing intention change history is full".into());
            }
        }
        let change = IntentChange {
            id: uuid::Uuid::new_v4().to_string(),
            before,
            after: after.clone(),
            undone: false,
        };
        self.intents.insert(after.id.clone(), after.clone());
        self.changes.insert(change.id.clone(), change.clone());
        Ok((after, change))
    }
    pub fn create(
        &mut self,
        id: &str,
        agent: &str,
        spec: IntentSpec,
        now: u64,
    ) -> Result<(StandingIntent, IntentChange), String> {
        if self.intents.len() >= MAX_INTENTS
            || self.intents.contains_key(id)
            || !bounded_id(id)
            || !bounded_id(agent)
            || !spec_valid(&spec)
            || spec.expires_at_ms <= now
            || spec.expires_at_ms > now.saturating_add(MAX_LIFETIME_MS)
        {
            return Err("Invalid or full standing intention registry".into());
        }
        self.commit_change(
            None,
            StandingIntent {
                id: id.into(),
                agent_id: agent.into(),
                spec,
                revision: 1,
                created_at_ms: now,
                updated_at_ms: now,
                cancelled: false,
                fires: vec![],
            },
        )
    }
    pub fn update(
        &mut self,
        id: &str,
        revision: u64,
        spec: IntentSpec,
        now: u64,
    ) -> Result<(StandingIntent, IntentChange), String> {
        let before = self
            .intents
            .get(id)
            .filter(|r| r.revision == revision && !r.cancelled)
            .cloned()
            .ok_or("Standing intention changed")?;
        if !spec_valid(&spec)
            || spec.expires_at_ms <= now
            || spec.expires_at_ms > now.saturating_add(MAX_LIFETIME_MS)
            || spec.max_fires < (before.fires.len() as u32)
            || before.fires.iter().any(|f| f.at_ms >= spec.expires_at_ms)
        {
            return Err("Invalid intention update".into());
        }
        let mut after = before.clone();
        after.spec = spec;
        after.revision = after.revision.checked_add(1).ok_or("Revision exhausted")?;
        after.updated_at_ms = now.max(before.updated_at_ms);
        self.commit_change(Some(before), after)
    }
    pub fn cancel(
        &mut self,
        id: &str,
        revision: u64,
        now: u64,
    ) -> Result<(StandingIntent, IntentChange), String> {
        let before = self
            .intents
            .get(id)
            .filter(|r| r.revision == revision && !r.cancelled)
            .cloned()
            .ok_or("Standing intention changed")?;
        let mut after = before.clone();
        after.cancelled = true;
        after.revision = after.revision.checked_add(1).ok_or("Revision exhausted")?;
        after.updated_at_ms = now.max(before.updated_at_ms);
        self.commit_change(Some(before), after)
    }
    /// Undo restores the exact saved owner fields and fire ledger; only the audit
    /// revision/time advance so a stale edit cannot acquire authority through ABA.
    pub fn undo(&mut self, id: &str, now: u64) -> Result<Option<StandingIntent>, String> {
        let change = self
            .changes
            .get(id)
            .filter(|c| !c.undone)
            .cloned()
            .ok_or("Change already undone or missing")?;
        if self.intents.get(&change.after.id) != Some(&change.after) {
            return Err("Intention changed after this edit; Undo refused".into());
        }
        let restored = match change.before {
            Some(mut before) => {
                before.revision = change
                    .after
                    .revision
                    .checked_add(1)
                    .ok_or("Revision exhausted")?;
                before.updated_at_ms = now.max(change.after.updated_at_ms);
                self.intents.insert(before.id.clone(), before.clone());
                Some(before)
            }
            None => {
                self.intents.remove(&change.after.id);
                None
            }
        };
        self.changes.get_mut(id).unwrap().undone = true;
        Ok(restored)
    }
    pub fn admit(&mut self, turn: &OwnerTurn<'_>) -> Result<Vec<IntentMatch>, String> {
        if !bounded_id(turn.agent_id)
            || !bounded_id(turn.run_id)
            || !bounded_id(turn.session_id)
            || turn.retry_key.is_some_and(|k| !bounded_id(k))
        {
            return Err("Invalid intention admission identity".into());
        }
        if turn.text.len() > MAX_TURN_BYTES
            || self.admissions.iter().any(|a| {
                a.agent_id == turn.agent_id
                    && (a.run_id == turn.run_id
                        || (turn.retry_key.is_some() && a.retry_key.as_deref() == turn.retry_key))
            })
        {
            return Ok(vec![]);
        }
        let mut rows: Vec<_> = self
            .intents
            .values()
            .filter(|r| {
                r.agent_id == turn.agent_id
                    && !r.cancelled
                    && r.created_at_ms <= turn.now_ms
                    && turn.now_ms < r.spec.expires_at_ms
                    && r.fires.len() < (r.spec.max_fires as usize)
                    && r.fires
                        .last()
                        .is_none_or(|f| turn.now_ms >= f.at_ms.saturating_add(r.spec.cooldown_ms))
                    && r.spec
                        .scope
                        .session_id
                        .as_deref()
                        .is_none_or(|id| id == turn.session_id)
                    && match r.spec.scope.sender {
                        SenderScope::Owner => true,
                        SenderScope::LocalOwner => turn.sender == OwnerSender::Local,
                        SenderScope::TelegramOwner => turn.sender == OwnerSender::Telegram,
                        SenderScope::DiscordOwner => turn.sender == OwnerSender::Discord,
                    }
                    && match &r.spec.condition {
                        IntentCondition::Keyword { phrase } => phrase_matches(turn.text, phrase),
                        IntentCondition::Event {
                            event: ConversationEvent::Message,
                        } => true,
                        IntentCondition::Event {
                            event: ConversationEvent::Started,
                        } => turn.conversation_started,
                    }
            })
            .map(|r| (r.created_at_ms, r.id.clone()))
            .collect();
        rows.sort();
        rows.truncate(3);
        if rows.is_empty() {
            return Ok(vec![]);
        }
        if self.admissions.len() >= MAX_ADMISSIONS {
            return Err("Standing intention admission history is full".into());
        }
        if rows
            .iter()
            .any(|(_, id)| self.intents[id].revision == u64::MAX)
        {
            return Err("Intention revision exhausted".into());
        }
        let mut matched = vec![];
        for (_, id) in rows {
            let r = self.intents.get_mut(&id).unwrap();
            r.fires.push(IntentFire {
                run_id: turn.run_id.into(),
                at_ms: turn.now_ms,
            });
            r.revision += 1;
            r.updated_at_ms = turn.now_ms.max(r.updated_at_ms);
            matched.push(IntentMatch {
                intent_id: id,
                instruction: r.spec.instruction.clone(),
            });
        }
        self.admissions.push(IntentAdmission {
            agent_id: turn.agent_id.into(),
            run_id: turn.run_id.into(),
            retry_key: turn.retry_key.map(str::to_owned),
            intent_ids: matched.iter().map(|m| m.intent_id.clone()).collect(),
        });
        Ok(matched)
    }
}
