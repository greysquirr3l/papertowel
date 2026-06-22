use base64::Engine as _;
use serde_json::{Value, json};

use crate::protocol::{DEFAULT_RESULT_TTL_MS, DEFAULT_PAGE_SIZE};

/// Built-in resource URIs shipped by the server.
pub const URI_CLEANUP_LATEST: &str = "papertowel://cleanup/latest";
pub const URI_CLEANUP_DEFERRED: &str = "papertowel://cleanup/deferred";
pub const URI_CLEANUP_TREND: &str = "papertowel://cleanup/trend";
pub const URI_CONFIG: &str = "papertowel://config";
pub const URI_RECIPES: &str = "papertowel://recipes";

const URI_CLEANUP_BY_NAME: &str = "papertowel://cleanup/{name}";

#[derive(Clone, Copy)]
struct StaticResource {
    uri: &'static str,
    name: &'static str,
    title: &'static str,
    description: &'static str,
    mime_type: &'static str,
}

// Static resources are sorted alphabetically by URI to give a deterministic
// order across invocations (recommended by `2026-07-28` SEP changelog).
const STATIC_RESOURCES: &[StaticResource] = &[
    StaticResource {
        uri: URI_CLEANUP_DEFERRED,
        name: "cleanup-deferred",
        title: "Cleanup deferred queue",
        description: "Findings deferred from the most recent cleanup cycle.",
        mime_type: "application/json",
    },
    StaticResource {
        uri: URI_CLEANUP_LATEST,
        name: "cleanup-latest",
        title: "Latest cleanup report",
        description: "Most recent cleanup assessment report persisted at .papertowel/cleanup/latest.json.",
        mime_type: "application/json",
    },
    StaticResource {
        uri: URI_CLEANUP_TREND,
        name: "cleanup-trend",
        title: "Cleanup trend summary",
        description: "Trend deltas between the previous and most recent cleanup reports.",
        mime_type: "application/json",
    },
    StaticResource {
        uri: URI_CONFIG,
        name: "project-config",
        title: "Project papertowel configuration",
        description: "Effective papertowel configuration for the current working directory.",
        mime_type: "application/toml",
    },
    StaticResource {
        uri: URI_RECIPES,
        name: "builtin-recipes",
        title: "Built-in detection recipes",
        description: "Recipes bundled with papertowel that drive scan and scrub detection.",
        mime_type: "application/toml",
    },
];

fn static_resource_to_json(resource: &StaticResource) -> Value {
    json!({
        "uri": resource.uri,
        "name": resource.name,
        "title": resource.title,
        "description": resource.description,
        "mimeType": resource.mime_type,
    })
}

/// Return the static resource templates exposed by this server.
pub fn resource_templates_list() -> Vec<Value> {
    vec![json!({
        "uriTemplate": URI_CLEANUP_BY_NAME,
        "name": "cleanup-report-by-name",
        "title": "Cleanup report by name",
        "description": "Read a previously persisted cleanup report by file name (e.g. latest, previous).",
        "mimeType": "application/json"
    })]
}

/// Return one page of resource descriptors with optional `nextCursor`.
///
/// Includes the `CacheableResult` fields required by the `2026-07-28` spec.
pub fn resources_list_page(cursor: Option<&str>) -> Value {
    let start = decode_cursor(cursor);
    let end = (start + DEFAULT_PAGE_SIZE).min(STATIC_RESOURCES.len());

    let page: Vec<Value> = STATIC_RESOURCES
        .iter()
        .skip(start)
        .take(end.saturating_sub(start))
        .map(static_resource_to_json)
        .collect();
    let next_cursor = if end < STATIC_RESOURCES.len() {
        Some(encode_cursor(end))
    } else {
        None
    };

    let mut result = json!({
        "resultType": crate::protocol::RESULT_TYPE_COMPLETE,
        "ttlMs": DEFAULT_RESULT_TTL_MS,
        "cacheScope": "public",
        "resources": page,
        "_meta": {
            "dev.greysquirr3l.papertowel/page-size": DEFAULT_PAGE_SIZE,
        }
    });
    if let Some(next) = next_cursor
        && let Some(obj) = result.as_object_mut()
    {
        obj.insert("nextCursor".to_owned(), Value::String(next));
    }
    result
}

/// Read the contents of a resource by URI.
///
/// Returns `None` for unknown URIs or when the resource is not available.
pub fn read_resource(uri: &str) -> Option<Value> {
    let contents = match uri {
        URI_CLEANUP_LATEST => read_cleanup_file("latest.json")?,
        URI_CLEANUP_DEFERRED => read_cleanup_file("deferred.json")?,
        URI_CLEANUP_TREND => read_trend_payload()?,
        URI_CONFIG => read_effective_config()?,
        URI_RECIPES => read_recipes_payload()?,
        other if other.starts_with("papertowel://cleanup/") => {
            let name = other.trim_start_matches("papertowel://cleanup/");
            if name.is_empty() || name.contains("..") || name.contains('/') {
                return None;
            }
            read_cleanup_file(&format!("{name}.json"))?
        }
        _ => return None,
    };

    Some(json!({
        "resultType": crate::protocol::RESULT_TYPE_COMPLETE,
        "ttlMs": DEFAULT_RESULT_TTL_MS,
        "cacheScope": "private",
        "contents": [contents],
    }))
}

fn text_contents(uri: &str, mime_type: &str, text: &str) -> Value {
    json!({
        "uri": uri,
        "mimeType": mime_type,
        "text": text,
    })
}

fn read_cleanup_file(file_name: &str) -> Option<Value> {
    let cwd = std::env::current_dir().ok()?;
    let state_dir = papertowel::cleanup::resolve_state_dir(&cwd.to_string_lossy(), None);
    let path = state_dir.join(file_name);
    let content = std::fs::read_to_string(&path).ok()?;
    Some(text_contents(
        &format!("papertowel://cleanup/{file_name}"),
        "application/json",
        &content,
    ))
}

fn read_trend_payload() -> Option<Value> {
    let cwd = std::env::current_dir().ok()?;
    let state_dir = papertowel::cleanup::resolve_state_dir(&cwd.to_string_lossy(), None);
    let status = papertowel::cleanup::read_status_report(&state_dir).ok()?;
    let json = serde_json::to_string_pretty(&status).ok()?;
    Some(text_contents(URI_CLEANUP_TREND, "application/json", &json))
}

fn read_effective_config() -> Option<Value> {
    let cwd = std::env::current_dir().ok()?;
    let (resolved, config, _ignore) = papertowel::config::resolve_config(&cwd).ok()?;
    let body = format!(
        "# resolved_path: {}\n{}",
        resolved.display(),
        toml::to_string(&config).unwrap_or_default(),
    );
    Some(text_contents(URI_CONFIG, "application/toml", &body))
}

fn read_recipes_payload() -> Option<Value> {
    use std::fmt::Write as _;
    let names = ["slop-vocabulary.toml", "phrase-patterns.toml", "comment-patterns.toml"];
    let mut out = String::new();
    for name in names {
        let Ok(body) = std::fs::read_to_string(format!(
            "{}/src/recipes/{name}",
            env!("CARGO_MANIFEST_DIR")
        )) else {
            continue;
        };
        let _ = writeln!(out, "# ── {name} ─────────────────────────────────────────");
        out.push_str(&body);
        out.push('\n');
    }
    if out.is_empty() {
        return None;
    }
    Some(text_contents(URI_RECIPES, "application/toml", &out))
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
    usize::from_le_bytes(arr).min(STATIC_RESOURCES.len())
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;

    #[test]
    fn list_page_includes_static_resources() {
        let page = resources_list_page(None);
        let includes_latest = page
            .get("resources")
            .and_then(Value::as_array)
            .is_some_and(|r| {
                r.iter().any(|item| {
                    item.get("uri")
                        .and_then(Value::as_str)
                        .is_some_and(|u| u == URI_CLEANUP_LATEST)
                })
            });
        assert!(includes_latest, "static list must include the cleanup-latest resource");
    }

    #[test]
    fn list_page_is_deterministically_ordered() {
        let page = resources_list_page(None);
        let resources_present = page.get("resources").and_then(Value::as_array).is_some();
        assert!(resources_present, "resources list must be present");
        let uris: Vec<String> = page
            .get("resources")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|r| r.get("uri").and_then(Value::as_str).map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        let mut sorted = uris.clone();
        sorted.sort();
        assert_eq!(uris, sorted, "resources must be sorted by URI");
    }

    #[test]
    fn list_page_includes_cacheable_fields() {
        let page = resources_list_page(None);
        assert_eq!(page.get("resultType"), Some(&Value::String("complete".into())));
        assert_eq!(page.get("cacheScope"), Some(&Value::String("public".into())));
        let ttl = page
            .get("ttlMs")
            .and_then(Value::as_u64)
            .is_some_and(|t| t > 0);
        assert!(ttl);
    }

    #[test]
    fn templates_list_includes_cleanup_by_name() {
        let templates = resource_templates_list();
        let includes = templates
            .iter()
            .any(|t| t.get("uriTemplate").and_then(Value::as_str) == Some(URI_CLEANUP_BY_NAME));
        assert!(includes);
    }

    #[test]
    fn unknown_uri_returns_none() {
        assert!(read_resource("papertowel://nope").is_none());
        assert!(read_resource("file:///etc/hosts").is_none());
    }
}