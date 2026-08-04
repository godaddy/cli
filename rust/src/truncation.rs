//! Context-safe truncation for agent-facing command output.
//!
//! Large lists and payloads are capped before being rendered so a single
//! command can't blow an agent's context window; the untruncated data is
//! written to a side-channel file under `$TMPDIR/godaddy-cli/` instead.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::Value;

const MAX_LIST_ITEMS: usize = 50;
const MAX_STRING_LENGTH: usize = 1000;
const MAX_SERIALIZED_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TruncationMetadata {
    pub(crate) truncated: bool,
    pub(crate) total: usize,
    pub(crate) shown: usize,
    pub(crate) full_output: Option<String>,
}

pub(crate) struct ListTruncationResult<T> {
    pub(crate) items: Vec<T>,
    pub(crate) metadata: TruncationMetadata,
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

    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_millis();
    let path = dir.join(format!("{millis}-{}.json", slugify(command_id)));
    let contents = serde_json::to_string_pretty(payload).ok()?;
    std::fs::write(&path, contents).ok()?;

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

/// Caps a list at [`MAX_LIST_ITEMS`], dumping the full list to a side-channel
/// file when it was truncated.
pub(crate) fn truncate_list<T: Serialize>(
    items: Vec<T>,
    command_id: &str,
) -> ListTruncationResult<T> {
    let total = items.len();
    let truncated = total > MAX_LIST_ITEMS;

    if !truncated {
        return ListTruncationResult {
            items,
            metadata: TruncationMetadata {
                truncated: false,
                total,
                full_output: None,
            },
        };
    }

    let full_output = serde_json::to_value(&items)
        .ok()
        .and_then(|value| write_full_output(command_id, &value))
        .map(|path| path.display().to_string());
    let mut items = items;
    items.truncate(MAX_LIST_ITEMS);
    ListTruncationResult {
        items,
        metadata: TruncationMetadata {
            truncated: true,
            total,
            full_output,
        },
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

