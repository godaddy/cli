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
