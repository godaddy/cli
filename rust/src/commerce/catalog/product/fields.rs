//! The flags the commands that change a product share, and what each accepts.
//! The API takes `type` and `status` as free strings; these are the values this
//! phase offers, checked when arguments are parsed so a typo is named before
//! anything is sent.

/// The kinds of product a merchant creates, in the form the API takes them. The
/// API also allows custom types, which this phase leaves out on purpose.
const KINDS: [&str; 2] = ["PHYSICAL", "DIGITAL"];

/// The statuses a command may set. Archiving has its own command, since it can
/// also archive a product's variants.
const STATUSES: [&str; 2] = ["DRAFT", "ACTIVE"];

/// A `--type` value as the API takes it. Case does not matter.
pub(super) fn parse_kind(value: &str) -> Result<String, String> {
    one_of(value, &KINDS, "a product type", "physical or digital")
}

/// A `--status` value as the API takes it. Case does not matter.
pub(super) fn parse_status(value: &str) -> Result<String, String> {
    one_of(
        value,
        &STATUSES,
        "a status a product can be set to",
        "draft or active; to archive a product use `product archive`",
    )
}

/// A `--label` value: the name shown to customers, which cannot be blank.
pub(super) fn parse_label(value: &str) -> Result<String, String> {
    let label = value.trim();
    if label.is_empty() {
        Err("the label cannot be blank".to_owned())
    } else {
        Ok(label.to_owned())
    }
}

/// The longest handle the API accepts.
const MAX_HANDLE: usize = 255;

/// A `--name` value: the product's short handle. The API takes 1 to 255 lowercase
/// letters, digits, `-` and `_`, and refuses anything else, so it is checked here
/// and a handle that would be refused is named before anything is sent, in a
/// preview as in a real run. Case is not changed for the person: a handle that
/// is not lowercase is refused and not quietly rewritten.
pub(super) fn parse_handle(value: &str) -> Result<String, String> {
    let handle = value.trim();
    let valid = !handle.is_empty()
        && handle.len() <= MAX_HANDLE
        && handle
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_'));
    if valid {
        Ok(handle.to_owned())
    } else {
        Err(format!(
            "{value:?} is not a valid handle; use 1 to {MAX_HANDLE} lowercase letters, digits, \
             `-` or `_`, for example classic-tee"
        ))
    }
}

fn one_of(value: &str, allowed: &[&str], what: &str, accepted: &str) -> Result<String, String> {
    let upper = value.trim().to_ascii_uppercase();
    if allowed.contains(&upper.as_str()) {
        Ok(upper)
    } else {
        Err(format!("{value:?} is not {what}; use {accepted}"))
    }
}

#[cfg(test)]
mod tests;
