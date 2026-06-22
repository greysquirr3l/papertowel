use serde_json::{Value, json};

use crate::protocol::RESULT_TYPE_COMPLETE;

/// Maximum number of completion values returned per response (spec limit).
const MAX_COMPLETION_VALUES: usize = 100;

/// Handle a `completion/complete` request by returning a list of values for the
/// referenced prompt or resource template argument.
///
/// The reference is a JSON object with a `type` field that distinguishes
/// `ref/prompt` (referenced by `name`) from `ref/resource` (referenced by
/// `uri`). The argument object carries the argument name and the current value
/// typed by the user.
pub fn complete(ref_value: &Value, argument: &Value) -> Result<Value, anyhow::Error> {
    let ref_kind = ref_value
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("invalid params: missing ref.type"))?;

    let arg_name = argument
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("invalid params: missing argument.name"))?;
    let arg_value = argument
        .get("value")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase();

    let pool: &[&str] = match (ref_kind, arg_name) {
        // Prompt: ai-fingerprint-audit
        ("ref/prompt", "path") => &[
            ".",
            "./src",
            "./crates",
            "./packages",
            "./services",
        ],
        ("ref/prompt", "min_severity") => &["low", "medium", "high"],

        // Prompt: cleanup-cycle
        ("ref/prompt", "min_confidence") => &["low", "medium", "high"],
        ("ref/prompt", "max_risk") => &["low", "medium", "high"],

        // Prompt: pre-commit-check
        ("ref/prompt", "dry_run") => &["true", "false"],

        // Prompt: refactor-budget
        ("ref/prompt", "budget_lines") => &["50", "100", "150", "200", "300", "500"],

        // Resource template papertowel://cleanup/{name}
        ("ref/resource", "name") => &["latest", "previous", "deferred"],

        _ => &[],
    };

    let mut matches: Vec<String> = pool
        .iter()
        .filter(|candidate| candidate.to_ascii_lowercase().contains(&arg_value))
        .map(|s| (*s).to_owned())
        .collect();
    matches.sort();
    matches.truncate(MAX_COMPLETION_VALUES);

    let total = pool.len();
    let has_more = matches.len() < total;

    Ok(json!({
        "resultType": RESULT_TYPE_COMPLETE,
        "completion": {
            "values": matches,
            "total": total,
            "hasMore": has_more,
        }
    }))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn completes_known_severity_argument() {
        let r#ref = json!({ "type": "ref/prompt", "name": "ai-fingerprint-audit" });
        let argument = json!({ "name": "min_severity", "value": "h" });
        let result = complete(&r#ref, &argument);
        let values_is_high = result.is_ok_and(|v| {
            v.get("completion")
                .and_then(|c| c.get("values"))
                .and_then(Value::as_array)
                .is_some_and(|arr| arr.iter().any(|item| item == &json!("high")))
        });
        assert!(values_is_high);
    }

    #[test]
    fn returns_empty_for_unknown_argument_name() {
        // "min_severity" matches a known arm; an unknown argument name should
        // produce an empty completion list.
        let r#ref = json!({ "type": "ref/prompt", "name": "ai-fingerprint-audit" });
        let argument = json!({ "name": "totally_unrecognised", "value": "h" });
        let result = complete(&r#ref, &argument);
        let values_empty = result.is_ok_and(|v| {
            v.get("completion")
                .and_then(|c| c.get("values"))
                .and_then(Value::as_array)
                .is_some_and(Vec::is_empty)
        });
        assert!(values_empty);
    }

    #[test]
    fn rejects_missing_ref_type() {
        let r#ref = json!({ "name": "ai-fingerprint-audit" });
        let argument = json!({ "name": "min_severity", "value": "" });
        assert!(complete(&r#ref, &argument).is_err());
    }
}