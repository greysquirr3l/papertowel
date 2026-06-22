use base64::Engine as _;
use serde_json::{Value, json};

use crate::protocol::{DEFAULT_PAGE_SIZE, DEFAULT_RESULT_TTL_MS, RESULT_TYPE_COMPLETE};

/// Argument names referenced by prompt definitions; exposed as constants so
/// completion logic and tests can reference them without stringly-typed drift.
pub const PROMPT_ARG_AUDIT_PATH: &str = "path";
pub const PROMPT_ARG_AUDIT_MIN_SEVERITY: &str = "min_severity";
pub const PROMPT_ARG_CLEANUP_PATH: &str = "path";
pub const PROMPT_ARG_CLEANUP_MIN_CONFIDENCE: &str = "min_confidence";
pub const PROMPT_ARG_CLEANUP_MAX_RISK: &str = "max_risk";
pub const PROMPT_ARG_PRECOMMIT_PATH: &str = "path";
pub const PROMPT_ARG_PRECOMMIT_DRY_RUN: &str = "dry_run";
pub const PROMPT_ARG_REFACTOR_PATH: &str = "path";
pub const PROMPT_ARG_REFACTOR_BUDGET: &str = "budget_lines";

#[derive(Clone)]
struct PromptArg {
    name: &'static str,
    description: &'static str,
    required: bool,
}

#[derive(Clone)]
struct PromptDef {
    name: &'static str,
    title: &'static str,
    description: &'static str,
    arguments: &'static [PromptArg],
    /// Returns the messages for the prompt given the arguments.
    build: fn(&Value) -> Vec<Value>,
}

const PROMPTS: &[PromptDef] = &[
    PromptDef {
        name: "ai-fingerprint-audit",
        title: "AI Fingerprint Audit",
        description: "Run a full papertowel audit: scan for findings, grade the codebase, and produce a prioritised scrub plan.",
        arguments: &[
            PromptArg {
                name: PROMPT_ARG_AUDIT_PATH,
                description: "Project root to audit. Defaults to the current working directory.",
                required: false,
            },
            PromptArg {
                name: PROMPT_ARG_AUDIT_MIN_SEVERITY,
                description: "Minimum severity threshold: low, medium, or high. Defaults to medium.",
                required: false,
            },
        ],
        build: build_audit_messages,
    },
    PromptDef {
        name: "cleanup-cycle",
        title: "Cleanup Cycle",
        description: "Drive the papertowel cleanup workflow: assess → status → apply with strict policy gates.",
        arguments: &[
            PromptArg {
                name: PROMPT_ARG_CLEANUP_PATH,
                description: "Project root to assess. Defaults to the current working directory.",
                required: false,
            },
            PromptArg {
                name: PROMPT_ARG_CLEANUP_MIN_CONFIDENCE,
                description: "Minimum confidence class for apply selection: low, medium, or high. Defaults to high.",
                required: false,
            },
            PromptArg {
                name: PROMPT_ARG_CLEANUP_MAX_RISK,
                description: "Maximum risk level allowed: low, medium, or high. Defaults to low.",
                required: false,
            },
        ],
        build: build_cleanup_messages,
    },
    PromptDef {
        name: "pre-commit-check",
        title: "Pre-Commit Check",
        description: "Run a focused scan/grade pass suitable for a pre-commit gate, with explicit pass/fail criteria.",
        arguments: &[
            PromptArg {
                name: PROMPT_ARG_PRECOMMIT_PATH,
                description: "Project root to scan. Defaults to the current working directory.",
                required: false,
            },
            PromptArg {
                name: PROMPT_ARG_PRECOMMIT_DRY_RUN,
                description: "Whether the scrub suggestion should be dry-run only. Defaults to true.",
                required: false,
            },
        ],
        build: build_precommit_messages,
    },
    PromptDef {
        name: "refactor-budget",
        title: "Refactor Budget",
        description: "Plan a refactor pass with a per-commit line budget that fits a human-scale change envelope.",
        arguments: &[
            PromptArg {
                name: PROMPT_ARG_REFACTOR_PATH,
                description: "Project root to refactor. Defaults to the current working directory.",
                required: false,
            },
            PromptArg {
                name: PROMPT_ARG_REFACTOR_BUDGET,
                description: "Soft cap on lines changed per proposed commit. Defaults to 200.",
                required: false,
            },
        ],
        build: build_refactor_messages,
    },
];

fn prompt_to_json(prompt: &PromptDef) -> Value {
    let arguments: Vec<Value> = prompt
        .arguments
        .iter()
        .map(|arg| {
            json!({
                "name": arg.name,
                "description": arg.description,
                "required": arg.required,
            })
        })
        .collect();
    json!({
        "name": prompt.name,
        "title": prompt.title,
        "description": prompt.description,
        "arguments": arguments,
    })
}

/// Return a single page of prompts (with cursor) for `prompts/list`.
///
/// Includes the `CacheableResult` fields (`ttlMs`, `cacheScope`) required by
/// the `2026-07-28` spec. Prompts are returned in a deterministic order
/// (alphabetical by name) so clients can rely on prompt list caching.
pub fn prompts_list_page(cursor: Option<&str>) -> Value {
    let start = decode_cursor(cursor);
    let end = (start + DEFAULT_PAGE_SIZE).min(PROMPTS.len());

    let page: Vec<Value> = PROMPTS
        .iter()
        .skip(start)
        .take(end.saturating_sub(start))
        .map(prompt_to_json)
        .collect();
    let next_cursor = if end < PROMPTS.len() {
        Some(encode_cursor(end))
    } else {
        None
    };

    let mut result = json!({
        "resultType": RESULT_TYPE_COMPLETE,
        "ttlMs": DEFAULT_RESULT_TTL_MS,
        "cacheScope": "public",
        "prompts": page,
        "_meta": {
            "dev.greysquirr3l.papertowel/page-size": DEFAULT_PAGE_SIZE,
        }
    });
    if let Some(next) = next_cursor {
        if let Some(obj) = result.as_object_mut() {
            obj.insert("nextCursor".to_owned(), Value::String(next));
        }
    }
    result
}

/// Return the rendered messages for a prompt name + arguments, or `None` if the
/// prompt name is not recognised.
pub fn get_prompt(name: &str, arguments: &Value) -> Option<Value> {
    let prompt = PROMPTS.iter().find(|p| p.name == name)?;
    let description = prompt.description.to_owned();
    let messages = (prompt.build)(arguments);
    Some(json!({
        "resultType": RESULT_TYPE_COMPLETE,
        "description": description,
        "messages": messages,
        "_meta": {
            "dev.greysquirr3l.papertowel/prompt-protocol-version": env!("CARGO_PKG_VERSION"),
        }
    }))
}

// ── prompt builders ─────────────────────────────────────────────────────────

fn opt_string<'a>(arguments: &'a Value, name: &str) -> Option<&'a str> {
    arguments.get(name).and_then(Value::as_str)
}

fn user_message(text: String) -> Value {
    json!({
        "role": "user",
        "content": { "type": "text", "text": text }
    })
}

fn build_audit_messages(arguments: &Value) -> Vec<Value> {
    let path = opt_string(arguments, PROMPT_ARG_AUDIT_PATH).unwrap_or(".");
    let severity = opt_string(arguments, PROMPT_ARG_AUDIT_MIN_SEVERITY).unwrap_or("medium");
    vec![user_message(format!(
        "You are auditing the project at `{path}` for AI-generated code fingerprints.\n\n\
         Workflow:\n\
         1. Call `papertowel_scan` with `path = \"{path}\"` and `min_severity = \"{severity}\"`.\n\
         2. Call `papertowel_grade` with `path = \"{path}\"` and `explain = true`.\n\
         3. Call `papertowel_scrub` for each high-impact file returned in step 1.\n\
         4. Summarise findings by `FindingCategory`, then propose a prioritised fix plan that\n\
            respects the persona profile configured for the project (default `night-owl`).\n\n\
         Do not call `papertowel_cleanup_apply` from this prompt; that lives behind the\n\
         `cleanup-cycle` prompt. Keep the response concise and focused on actionable steps."
    ))]
}

fn build_cleanup_messages(arguments: &Value) -> Vec<Value> {
    let path = opt_string(arguments, PROMPT_ARG_CLEANUP_PATH).unwrap_or(".");
    let min_conf = opt_string(arguments, PROMPT_ARG_CLEANUP_MIN_CONFIDENCE).unwrap_or("high");
    let max_risk = opt_string(arguments, PROMPT_ARG_CLEANUP_MAX_RISK).unwrap_or("low");
    vec![user_message(format!(
        "Run the papertowel cleanup cycle against `{path}`.\n\n\
         Steps:\n\
         1. Call `papertowel_cleanup_assess` with `path = \"{path}\"`.\n\
         2. Call `papertowel_cleanup_status` to confirm the persisted report is in place.\n\
         3. Dry-run `papertowel_cleanup_apply` with `report = <latest.json path>`,\n\
            `min_confidence = \"{min_conf}\"`, `max_risk = \"{max_risk}\"`, `dry_run = true`, `ci = true`.\n\
         4. If dry-run validation passes, repeat step 3 with `dry_run = false` after explicit\n\
            user confirmation. Never bypass the policy gates.\n\n\
         Surface any `blocked` items from the apply result and explain the gate that fired."
    ))]
}

fn build_precommit_messages(arguments: &Value) -> Vec<Value> {
    let path = opt_string(arguments, PROMPT_ARG_PRECOMMIT_PATH).unwrap_or(".");
    let dry_run = opt_string(arguments, PROMPT_ARG_PRECOMMIT_DRY_RUN).unwrap_or("true");
    vec![user_message(format!(
        "Run a focused papertowel pre-commit gate against `{path}`.\n\n\
         1. Call `papertowel_scan` with `path = \"{path}\"` and `min_severity = \"high\"`.\n\
         2. Call `papertowel_grade` with `path = \"{path}\"`.\n\
         3. If the grade is worse than B, call `papertowel_scrub` on the top three files\n\
            (`dry_run = {dry_run}`) and present the suggestions as a checklist.\n\n\
         Block the commit if any high-severity findings remain. Otherwise report pass with a\n\
         one-line summary that includes the grade and finding count."
    ))]
}

fn build_refactor_messages(arguments: &Value) -> Vec<Value> {
    let path = opt_string(arguments, PROMPT_ARG_REFACTOR_PATH).unwrap_or(".");
    let budget = opt_string(arguments, PROMPT_ARG_REFACTOR_BUDGET).unwrap_or("200");
    vec![user_message(format!(
        "Plan a refactor pass for `{path}` with a soft per-commit line budget of {budget}.\n\n\
         1. Call `papertowel_scan` to enumerate findings, grouped by file.\n\
         2. Propose a sequence of commits — each commit must:\n\
            - Touch at most {budget} lines (soft cap, justify overruns).\n\
            - Have a tightly scoped conventional-commit message.\n\
            - Pass `papertowel_scan` at `min_severity = \"high\"` after each step.\n\
         3. End with a `papertowel_grade` summary showing the delta from the baseline.\n\n\
         Avoid the `papertowel_cleanup_*` tools in this prompt; this is a human-scale refactor\n\
         plan, not the policy-gated cleanup cycle."
    ))]
}

// ── cursor helpers ──────────────────────────────────────────────────────────

fn encode_cursor(index: usize) -> String {
    let bytes = index.to_le_bytes();
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn decode_cursor(cursor: Option<&str>) -> usize {
    let Some(cursor) = cursor else {
        return 0;
    };
    let Ok(bytes) = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(cursor) else {
        return 0;
    };
    if bytes.len() != std::mem::size_of::<usize>() {
        return 0;
    }
    let Ok(arr) = <[u8; std::mem::size_of::<usize>()]>::try_from(bytes.as_slice()) else {
        return 0;
    };
    usize::from_le_bytes(arr).min(PROMPTS.len())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn list_page_cursors_round_trip() {
        let first = prompts_list_page(None);
        let prompts_present_and_nonempty = first
            .get("prompts")
            .and_then(Value::as_array)
            .is_some_and(|p| !p.is_empty());
        assert!(prompts_present_and_nonempty);

        // A small enough page list should not include nextCursor.
        let all_returned = first
            .get("prompts")
            .and_then(Value::as_array)
            .is_some_and(|p| p.len() >= PROMPTS.len());
        if !all_returned {
            assert!(first.get("nextCursor").is_some());
        }
    }

    #[test]
    fn get_prompt_audit_returns_messages() {
        let result = get_prompt("ai-fingerprint-audit", &json!({}));
        let contains_scan = result.is_some_and(|v| {
            v.get("messages")
                .and_then(Value::as_array)
                .and_then(|msgs| msgs.first())
                .and_then(|m| m.get("content"))
                .and_then(|c| c.get("text"))
                .and_then(Value::as_str)
                .is_some_and(|s| s.contains("papertowel_scan"))
        });
        assert!(contains_scan);
    }

    #[test]
    fn get_prompt_unknown_returns_none() {
        assert!(get_prompt("nope", &json!({})).is_none());
    }
}