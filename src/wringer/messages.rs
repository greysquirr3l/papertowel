use std::collections::HashMap;

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

use crate::profile::persona::{CommitMessageStyle, PersonaProfile};
use crate::wringer::queue::{PendingCommit, QueuePlan, ReplayAction};

pub const COMPONENT_NAME: &str = "messages";

const CONVENTIONAL_VERBS: &[&str] = &["feat", "fix", "refactor", "chore", "docs", "test"];
const LAZY_MESSAGES: &[&str] = &[
    "wip",
    "fix stuff",
    "cleanup",
    "small tweak",
    "more changes",
    "fix the fix",
];
const MILD_PROFANITY: &[&str] = &["damn", "ugh"];
const ASCII_EMOJIS: &[&str] = &[":)", ":/", ":D"];

#[derive(Debug, Clone)]
pub struct MessageContext {
    pub source_oids: Vec<String>,
    pub changed_files: Vec<String>,
    pub original_messages: Vec<String>,
    pub action: ReplayAction,
}

pub fn humanize_queue_plan(
    plan: &mut QueuePlan,
    pending: &[PendingCommit],
    persona: &PersonaProfile,
) {
    let mut rng = StdRng::seed_from_u64(0x00C0_FFEE_u64);
    humanize_queue_plan_with_rng(plan, pending, persona, &mut rng);
}

pub fn humanize_queue_plan_with_seed(
    plan: &mut QueuePlan,
    pending: &[PendingCommit],
    persona: &PersonaProfile,
    seed: u64,
) {
    let mut rng = StdRng::seed_from_u64(seed);
    humanize_queue_plan_with_rng(plan, pending, persona, &mut rng);
}

fn humanize_queue_plan_with_rng(
    plan: &mut QueuePlan,
    pending: &[PendingCommit],
    persona: &PersonaProfile,
    rng: &mut impl Rng,
) {
    let by_oid: HashMap<&str, &PendingCommit> =
        pending.iter().map(|c| (c.oid.as_str(), c)).collect();

    for entry in &mut plan.entries {
        let mut files: Vec<String> = Vec::new();
        let mut originals: Vec<String> = Vec::new();

        for oid in &entry.source_oids {
            if let Some(commit) = by_oid.get(oid.as_str()) {
                files.extend(commit.changed_files.iter().cloned());
                originals.push(commit.message.clone());
            }
        }

        let context = MessageContext {
            source_oids: entry.source_oids.clone(),
            changed_files: files,
            original_messages: originals,
            action: entry.action.clone(),
        };
        entry.message = generate_humanized_message(&context, persona, rng);
    }
}

pub fn generate_humanized_message(
    context: &MessageContext,
    persona: &PersonaProfile,
    rng: &mut impl Rng,
) -> String {
    if should_fire(persona.messages.wip_frequency, rng) {
        let pick = rng.gen_range(0..LAZY_MESSAGES.len());
        if let Some(msg) = LAZY_MESSAGES.get(pick) {
            return apply_entropy((*msg).to_owned(), persona, rng);
        }
    }

    let base = match persona.messages.style {
        CommitMessageStyle::Conventional => conventional_message(context, rng),
        CommitMessageStyle::Lazy => lazy_message(rng),
        CommitMessageStyle::Mixed => {
            if rng.gen_bool(0.60) {
                conventional_message(context, rng)
            } else {
                lazy_message(rng)
            }
        }
    };

    apply_entropy(base, persona, rng)
}

fn conventional_message(context: &MessageContext, rng: &mut impl Rng) -> String {
    let scope = infer_scope(&context.changed_files);
    let verb = pick_conventional_verb(&context.action, rng);
    let subject = summarize_subject(
        &context.original_messages,
        context.action == ReplayAction::Squash,
    );
    format!("{verb}({scope}): {subject}")
}

fn pick_conventional_verb(action: &ReplayAction, rng: &mut impl Rng) -> &'static str {
    match action {
        ReplayAction::Split => "refactor",
        ReplayAction::Squash => "feat",
        ReplayAction::Replay => {
            let pick = rng.gen_range(0..CONVENTIONAL_VERBS.len());
            CONVENTIONAL_VERBS.get(pick).copied().unwrap_or("chore")
        }
    }
}

fn lazy_message(rng: &mut impl Rng) -> String {
    let pick = rng.gen_range(0..LAZY_MESSAGES.len());
    LAZY_MESSAGES.get(pick).copied().unwrap_or("wip").to_owned()
}

fn infer_scope(files: &[String]) -> String {
    let mut scopes: Vec<String> = files
        .iter()
        .filter_map(|file| file.split('/').next())
        .map(ToOwned::to_owned)
        .collect();
    scopes.sort_unstable();
    scopes.dedup();

    match scopes.as_slice() {
        [] => String::from("core"),
        [single] => single.clone(),
        [first, second, ..] => format!("{first}+{second}"),
    }
}

fn summarize_subject(messages: &[String], squashed: bool) -> String {
    let first = messages.first().map_or("update code", String::as_str);
    let first_line = first.lines().next().unwrap_or("update code").trim();

    let normalized = normalize_subject(first_line);

    if squashed {
        return format!("{normalized} and related updates");
    }
    normalized
}

fn normalize_subject(input: &str) -> String {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return String::from("update code");
    }

    // Strip common conventional prefixes so we do not produce nested patterns
    // like "feat(scope): feat(scope): ...".
    if let Some((_, rest)) = trimmed.split_once(':') {
        let prefix = trimmed.split(':').next().unwrap_or_default();
        let conventional_like = prefix.contains('(')
            || matches!(
                prefix,
                "feat" | "fix" | "chore" | "docs" | "refactor" | "test" | "perf"
            );
        if conventional_like {
            let cleaned = rest.trim();
            if !cleaned.is_empty() {
                return cleaned.to_owned();
            }
        }
    }

    trimmed.to_owned()
}

fn apply_entropy(mut message: String, persona: &PersonaProfile, rng: &mut impl Rng) -> String {
    if should_fire(0.05, rng) {
        return if should_fire(0.50, rng) {
            String::from("wip")
        } else {
            String::from("tmp")
        };
    }

    if should_fire(persona.messages.typo_rate, rng) {
        message = inject_typo(&message, rng);
    }

    if should_fire(persona.messages.profanity_frequency, rng) {
        let pick = rng.gen_range(0..MILD_PROFANITY.len());
        let profanity = MILD_PROFANITY.get(pick).copied().unwrap_or("ugh");
        message = format!("{message} {profanity}");
    }

    if should_fire(0.20, rng) {
        message = random_capitalization(message, rng);
    }

    if should_fire(persona.messages.emoji_rate, rng) {
        let pick = rng.gen_range(0..ASCII_EMOJIS.len());
        let emoji = ASCII_EMOJIS.get(pick).copied().unwrap_or(":)");
        message = format!("{message} {emoji}");
    }

    message
}

fn should_fire(rate: f32, rng: &mut impl Rng) -> bool {
    if !rate.is_finite() || rate <= 0.0 {
        return false;
    }
    if rate >= 1.0 {
        return true;
    }
    rng.gen_bool(f64::from(rate))
}

fn inject_typo(input: &str, rng: &mut impl Rng) -> String {
    let mut chars: Vec<char> = input.chars().collect();
    if chars.len() < 4 {
        return input.to_owned();
    }

    let idx = rng.gen_range(1..chars.len() - 1);
    if should_fire(0.50, rng) {
        // Drop a character.
        chars.remove(idx);
    } else if idx + 1 < chars.len() {
        // Swap adjacent characters.
        chars.swap(idx, idx + 1);
    }

    chars.into_iter().collect()
}

fn random_capitalization(input: String, rng: &mut impl Rng) -> String {
    if should_fire(0.50, rng) {
        return input.to_lowercase();
    }
    let mut chars = input.chars();
    let Some(first) = chars.next() else {
        return input;
    };
    let head: String = first.to_uppercase().collect();
    let tail: String = chars.collect();
    format!("{head}{tail}")
}

#[cfg(test)]
mod tests {
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    use super::{MessageContext, generate_humanized_message, humanize_queue_plan_with_seed};
    use crate::profile::persona::{CommitMessageStyle, PersonaMessages, PersonaProfile};
    use crate::wringer::queue::{PendingCommit, QueueEntry, QueuePlan, ReplayAction};
    use chrono::Utc;

    fn persona_with(style: CommitMessageStyle) -> PersonaProfile {
        let mut profile = PersonaProfile::built_in_profiles()[0].clone();
        profile.messages = PersonaMessages {
            style,
            wip_frequency: 0.0,
            profanity_frequency: 0.0,
            typo_rate: 0.0,
            emoji_rate: 0.0,
        };
        profile
    }

    #[test]
    fn conventional_style_uses_scope_prefix() {
        let profile = persona_with(CommitMessageStyle::Conventional);
        let context = MessageContext {
            source_oids: vec![String::from("abc")],
            changed_files: vec![String::from("src/wringer/queue.rs")],
            original_messages: vec![String::from("add replay planner")],
            action: ReplayAction::Replay,
        };
        let mut rng = StdRng::seed_from_u64(7);

        let msg = generate_humanized_message(&context, &profile, &mut rng);
        assert!(msg.contains("(src):") || msg.contains("(wringer):") || msg.contains("(core):"));
    }

    #[test]
    fn lazy_style_picks_short_message() {
        let profile = persona_with(CommitMessageStyle::Lazy);
        let context = MessageContext {
            source_oids: vec![String::from("abc")],
            changed_files: vec![String::from("src/wringer/queue.rs")],
            original_messages: vec![String::from("feat(queue): add planner")],
            action: ReplayAction::Replay,
        };
        let mut rng = StdRng::seed_from_u64(1);

        let msg = generate_humanized_message(&context, &profile, &mut rng);
        assert!(
            msg == "wip"
                || msg == "fix stuff"
                || msg == "cleanup"
                || msg == "small tweak"
                || msg == "more changes"
                || msg == "fix the fix"
        );
    }

    #[test]
    fn entropy_injection_changes_message_when_rates_are_high() {
        let mut profile = persona_with(CommitMessageStyle::Conventional);
        profile.messages.typo_rate = 1.0;
        profile.messages.profanity_frequency = 1.0;
        profile.messages.emoji_rate = 1.0;

        let context = MessageContext {
            source_oids: vec![String::from("abc")],
            changed_files: vec![String::from("src/main.rs")],
            original_messages: vec![String::from("add thing")],
            action: ReplayAction::Replay,
        };
        let mut rng = StdRng::seed_from_u64(99);

        let msg = generate_humanized_message(&context, &profile, &mut rng);
        assert!(msg.contains("damn") || msg.contains("ugh"));
        assert!(msg.contains(":)") || msg.contains(":/") || msg.contains(":D"));
    }

    #[test]
    fn humanize_queue_plan_updates_entry_messages() -> Result<(), Box<dyn std::error::Error>> {
        let profile = persona_with(CommitMessageStyle::Conventional);
        let pending = vec![PendingCommit {
            oid: String::from("abc"),
            message: String::from("feat(queue): initial planner"),
            author: String::from("dev"),
            timestamp: Utc::now(),
            changed_files: vec![String::from("src/wringer/queue.rs")],
        }];

        let mut plan = QueuePlan {
            sync_point: None,
            persona_name: profile.name.clone(),
            entries: vec![QueueEntry {
                source_oids: vec![String::from("abc")],
                message: String::from("placeholder"),
                target_time: Utc::now(),
                action: ReplayAction::Replay,
                completed: false,
            }],
            generated_at: Utc::now(),
        };

        humanize_queue_plan_with_seed(&mut plan, &pending, &profile, 42);
        let entry = plan.entries.first().ok_or("expected queue entry")?;
        assert_ne!(entry.message, "placeholder");
        Ok(())
    }
}
