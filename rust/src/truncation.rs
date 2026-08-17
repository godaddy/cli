//! Context-safe truncation for agent-facing command output.
//!
//! Large lists and payloads are capped before being rendered so a single
//! command can't blow an agent's context window; the untruncated data is
//! written to a side-channel file under `$TMPDIR/godaddy-cli/` instead.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

const MAX_STRING_LENGTH: usize = 1000;
const MAX_SERIALIZED_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TruncationMetadata {
    pub(crate) truncated: bool,
    pub(crate) total: usize,
    pub(crate) shown: usize,
    pub(crate) full_output: Option<String>,
}

pub(crate) struct PayloadTruncationResult {
    pub(crate) value: Value,
    pub(crate) metadata: Option<TruncationMetadata>,
}

fn slugify(command_id: &str) -> String {
    command_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '-'
            }
        })
        .collect()
}

fn estimate_bytes(value: &Value) -> usize {
    serde_json::to_string(value).map(|s| s.len()).unwrap_or(0)
}

/// Best-effort dump of the untruncated payload to a `0600` file under a
/// `0700` directory. Returns `None` if the filesystem write fails, in which
/// case the caller simply omits `full_output` from the truncation metadata.
fn write_full_output(command_id: &str, payload: &Value) -> Option<PathBuf> {
    let dir = std::env::temp_dir().join("godaddy-cli");
    std::fs::create_dir_all(&dir).ok()?;
    #[cfg(unix)]
    {
        use std::fs::Permissions;
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, Permissions::from_mode(0o700)).ok();
    }

    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_millis();
    let path = dir.join(format!("{millis}-{}.json", slugify(command_id)));
    let contents = serde_json::to_string_pretty(payload).ok()?;
    std::fs::write(&path, contents).ok()?;
    #[cfg(unix)]
    {
        use std::fs::Permissions;
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, Permissions::from_mode(0o600)).ok();
    }

    Some(path)
}

fn truncate_strings(value: &Value) -> Value {
    match value {
        Value::String(s) if s.chars().count() > MAX_STRING_LENGTH => {
            let truncated: String = s.chars().take(MAX_STRING_LENGTH).collect();
            Value::String(format!("{truncated}...(truncated)"))
        }
        Value::Array(items) => Value::Array(items.iter().map(truncate_strings).collect()),
        Value::Object(obj) => Value::Object(
            obj.iter()
                .map(|(key, nested)| (key.clone(), truncate_strings(nested)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// Truncates long strings within `value`, then falls back to a `{ truncated,
/// summary }` stub if the result still exceeds [`MAX_SERIALIZED_BYTES`].
/// Dumps the original, untouched `value` to a side-channel file whenever
/// truncation occurred.
pub(crate) fn protect_payload(value: Value, command_id: &str) -> PayloadTruncationResult {
    let total_bytes = estimate_bytes(&value);
    let mut candidate = truncate_strings(&value);
    let mut shown_bytes = estimate_bytes(&candidate);
    let mut truncated = total_bytes != shown_bytes;

    if shown_bytes > MAX_SERIALIZED_BYTES {
        truncated = true;
        candidate = serde_json::json!({
            "truncated": true,
            "summary": "Output too large for inline payload",
        });
        shown_bytes = estimate_bytes(&candidate);
    }

    if !truncated {
        return PayloadTruncationResult {
            value: candidate,
            metadata: None,
        };
    }

    let full_output = write_full_output(command_id, &value).map(|path| path.display().to_string());
    PayloadTruncationResult {
        value: candidate,
        metadata: Some(TruncationMetadata {
            truncated: true,
            total: total_bytes,
            shown: shown_bytes,
            full_output,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn truncate_strings_truncates_long_values_recursively() {
        let long = "a".repeat(1500);
        let value = json!({ "nested": [long.clone()] });
        let truncated = truncate_strings(&value);
        let nested = truncated["nested"][0].as_str().expect("string");
        assert!(nested.ends_with("...(truncated)"));
        assert_eq!(nested.len(), MAX_STRING_LENGTH + "...(truncated)".len());
    }

    #[test]
    fn protect_payload_passes_through_small_values_unchanged() {
        let value = json!({ "name": "short" });
        let result = protect_payload(value.clone(), "test-small");
        assert_eq!(result.value, value);
        assert!(result.metadata.is_none());
    }

    #[test]
    fn protect_payload_truncates_long_strings_and_reports_metadata() {
        let long = "a".repeat(1500);
        let value = json!({ "name": long });
        let result = protect_payload(value, "test-strings");
        let metadata = result.metadata.expect("metadata present");
        assert!(metadata.truncated);
        assert!(metadata.full_output.is_some());
    }

    #[test]
    fn protect_payload_falls_back_to_summary_when_still_too_large() {
        let big_array: Vec<String> = (0..2000).map(|i| format!("item-{i}")).collect();
        let value = json!({ "items": big_array });
        let result = protect_payload(value, "test-huge");
        let metadata = result.metadata.expect("metadata present");
        assert!(metadata.truncated);
        assert_eq!(result.value["truncated"], json!(true));
        assert!(result.value.get("summary").is_some());
        let full_output = metadata.full_output.expect("full_output path");
        std::fs::remove_file(full_output).ok();
    }
}
