use std::future::Future;

use cli_engine::{CliCoreError, CommandContext, Result};

use super::client::make_client;
use super::errors::{api_error, format_api_error};
use super::validation::{
    validate_domain_name, validate_nameserver_hosts, validate_operation_id, validate_quote_token,
    validate_tld,
};

/// Prompt until the user enters a valid TLD or cancels.
pub(crate) fn prompt_validated_tld(prompt: &str) -> Result<String> {
    use dialoguer::Input;

    let input: String = Input::new()
        .with_prompt(prompt)
        .validate_with(|input: &String| -> std::result::Result<(), String> {
            validate_tld(input).map(|_| ()).map_err(|e| e.to_string())
        })
        .interact_text()
        .map_err(|e| CliCoreError::message(format!("prompt cancelled: {e}")))?;
    validate_tld(&input)
}

/// Validate `--tld` values, re-prompting interactively when input is invalid.
#[allow(clippy::print_stderr)]
pub(crate) fn resolve_tlds(
    ctx: &CommandContext,
    raw: Vec<String>,
    prompt: &str,
) -> Result<Vec<String>> {
    if raw.is_empty() {
        if ctx.is_interactive() {
            return Ok(vec![prompt_validated_tld(prompt)?]);
        }
        return Err(CliCoreError::message("at least one --tld is required"));
    }
    match raw
        .iter()
        .map(|t| validate_tld(t))
        .collect::<Result<Vec<_>>>()
    {
        Ok(tlds) => Ok(tlds),
        Err(e) if ctx.is_interactive() => {
            eprintln!("  {e}");
            Ok(vec![prompt_validated_tld(prompt)?])
        }
        Err(e) => Err(e),
    }
}

/// User-facing hint when the agreements API rejects a TLD the user can retry.
pub(crate) fn recoverable_tld_api_error(status: u16, body: &str) -> Option<String> {
    if status != 422 {
        return None;
    }
    #[derive(serde::Deserialize)]
    struct CodeMessage {
        #[serde(default)]
        code: String,
        #[serde(default)]
        message: String,
    }
    let parsed = serde_json::from_str::<CodeMessage>(body).ok()?;
    if parsed.code != "UNSUPPORTED_TLD" {
        return None;
    }
    if parsed.message.is_empty() {
        Some("that TLD is not supported; try another (e.g. com)".to_owned())
    } else {
        Some(format!(
            "that TLD is not supported: {} — try another (e.g. com)",
            parsed.message
        ))
    }
}

/// User-facing hint when an operation lookup fails and the user can retry.
pub(crate) fn recoverable_operation_api_error(status: u16, _body: &str) -> Option<String> {
    if status == 404 {
        Some("no operation found with that ID — check the operation ID and try again".to_owned())
    } else {
        None
    }
}

/// User-facing hint when a domain lookup fails and the user can retry.
pub(crate) fn recoverable_domain_lookup_api_error(status: u16, _body: &str) -> Option<String> {
    match status {
        404 => Some("that domain was not found — check the name and try again".to_owned()),
        422 => Some("that domain could not be looked up — check the name and try again".to_owned()),
        _ => None,
    }
}

/// Prompt until the user enters a valid operation ID or cancels.
pub(crate) fn prompt_validated_operation_id(prompt: &str) -> Result<String> {
    use dialoguer::Input;

    let input: String = Input::new()
        .with_prompt(prompt)
        .validate_with(|input: &String| -> std::result::Result<(), String> {
            validate_operation_id(input)
                .map(|_| ())
                .map_err(|e| e.to_string())
        })
        .interact_text()
        .map_err(|e| CliCoreError::message(format!("prompt cancelled: {e}")))?;
    validate_operation_id(&input)
}

/// Validate an operation ID, re-prompting interactively when input is invalid.
#[allow(clippy::print_stderr)]
pub(crate) fn resolve_operation_id(
    ctx: &CommandContext,
    raw: &str,
    prompt: &str,
) -> Result<String> {
    match validate_operation_id(raw) {
        Ok(id) => Ok(id),
        Err(e) if ctx.is_interactive() => {
            eprintln!("  {e}");
            prompt_validated_operation_id(prompt)
        }
        Err(e) => Err(e),
    }
}

/// Prompt until the user enters a non-empty quote token or cancels.
pub(crate) fn prompt_validated_quote_token(prompt: &str) -> Result<String> {
    use dialoguer::Input;

    let input: String = Input::new()
        .with_prompt(prompt)
        .validate_with(|input: &String| -> std::result::Result<(), String> {
            validate_quote_token(input)
                .map(|_| ())
                .map_err(|e| e.to_string())
        })
        .interact_text()
        .map_err(|e| CliCoreError::message(format!("prompt cancelled: {e}")))?;
    validate_quote_token(&input)
}

/// Resolve a quote token, re-prompting interactively when missing or invalid.
#[allow(clippy::print_stderr)]
pub(crate) fn resolve_quote_token(
    ctx: &CommandContext,
    raw: Option<String>,
    prompt: &str,
) -> Result<String> {
    match raw {
        Some(token) => match validate_quote_token(&token) {
            Ok(token) => Ok(token),
            Err(e) if ctx.is_interactive() => {
                eprintln!("  {e}");
                prompt_validated_quote_token(prompt)
            }
            Err(e) => Err(e),
        },
        None if ctx.is_interactive() => prompt_validated_quote_token(prompt),
        None => Err(CliCoreError::message(
            "--quote-token is required.\n\
             \n  Get one with `gddy domain quote <domain>`, or use the interactive \
             wizard:\n\
             \n    gddy domain register\n\
             \n  Then purchase with:\n\
             \n    gddy domain purchase --quote-token <token> --agree --confirm",
        )),
    }
}

/// Validate optional `--tlds` filters, re-prompting interactively when invalid.
#[allow(clippy::print_stderr)]
pub(crate) fn resolve_optional_tlds(
    ctx: &CommandContext,
    raw: Vec<String>,
    prompt: &str,
) -> Result<Vec<String>> {
    if raw.is_empty() {
        return Ok(vec![]);
    }
    match raw
        .iter()
        .map(|t| validate_tld(t))
        .collect::<Result<Vec<_>>>()
    {
        Ok(tlds) => Ok(tlds),
        Err(e) if ctx.is_interactive() => {
            eprintln!("  {e}");
            Ok(vec![prompt_validated_tld(prompt)?])
        }
        Err(e) => Err(e),
    }
}

/// Prompt until the user enters a valid domain name or cancels.
pub(crate) fn prompt_validated_domain_name(prompt: &str) -> Result<String> {
    use dialoguer::Input;

    let input: String = Input::new()
        .with_prompt(prompt)
        .validate_with(|input: &String| -> std::result::Result<(), String> {
            validate_domain_name(input)
                .map(|_| ())
                .map_err(|e| e.to_string())
        })
        .interact_text()
        .map_err(|e| CliCoreError::message(format!("prompt cancelled: {e}")))?;
    validate_domain_name(&input)
}

/// Validate a domain name, re-prompting interactively when input is invalid.
///
/// Used by leaf commands whose positional `DOMAIN` arg may be supplied via
/// cli-engine's missing-arg recovery (which does not validate format).
#[allow(clippy::print_stderr)] // interactive user-facing feedback, not diagnostic logging
pub(crate) fn resolve_domain_name(ctx: &CommandContext, raw: &str, prompt: &str) -> Result<String> {
    match validate_domain_name(raw) {
        Ok(domain) => Ok(domain),
        Err(e) if ctx.is_interactive() => {
            eprintln!("  {e}");
            prompt_validated_domain_name(prompt)
        }
        Err(e) => Err(e),
    }
}

/// Prompt until the user enters a valid nameserver host or cancels.
pub(crate) fn prompt_validated_nameserver(prompt: &str) -> Result<String> {
    use dialoguer::Input;

    let input: String = Input::new()
        .with_prompt(prompt)
        .validate_with(|input: &String| -> std::result::Result<(), String> {
            validate_domain_name(input)
                .map(|_| ())
                .map_err(|e| format!("--nameserver {e}"))
        })
        .interact_text()
        .map_err(|e| CliCoreError::message(format!("prompt cancelled: {e}")))?;
    validate_domain_name(&input)
}

/// Resolve `--nameserver` hosts, re-prompting interactively when missing or invalid.
#[allow(clippy::print_stderr)]
pub(crate) fn resolve_nameserver_hosts(
    ctx: &CommandContext,
    raw: Vec<String>,
    prompt: &str,
) -> Result<Vec<String>> {
    if raw.is_empty() {
        if ctx.is_interactive() {
            return Ok(vec![prompt_validated_nameserver(prompt)?]);
        }
        return Err(CliCoreError::message(
            "at least one --nameserver is required",
        ));
    }
    match validate_nameserver_hosts(raw) {
        Ok(hosts) => Ok(hosts),
        Err(e) if ctx.is_interactive() => {
            eprintln!("  {e}");
            Ok(vec![prompt_validated_nameserver(prompt)?])
        }
        Err(e) => Err(e),
    }
}

/// Re-run a domain-scoped API call, re-prompting for the domain on recoverable errors.
#[allow(clippy::print_stderr)]
pub(crate) async fn fetch_with_domain_retry<T, F, Fut>(
    ctx: &CommandContext,
    initial_domain: String,
    prompt: &str,
    action: &str,
    debug: bool,
    fetch: F,
) -> Result<T>
where
    F: Fn(domains_client::Client, String) -> Fut,
    Fut: Future<Output = std::result::Result<T, domains_client::Error<()>>>,
{
    let client = make_client(ctx).await?;
    let mut domain = initial_domain;
    loop {
        match fetch(client.clone(), domain.clone()).await {
            Ok(value) => return Ok(value),
            Err(domains_client::Error::UnexpectedResponse(resp)) if ctx.is_interactive() => {
                let status = resp.status().as_u16();
                let status_display = resp.status().to_string();
                let request_id = resp
                    .headers()
                    .get("x-request-id")
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_owned);
                let body = resp.text().await.unwrap_or_default();
                if let Some(hint) = recoverable_domain_lookup_api_error(status, &body) {
                    eprintln!("  {hint}");
                    domain = prompt_validated_domain_name(prompt)?;
                    continue;
                }
                return Err(CliCoreError::message(format_api_error(
                    action,
                    status,
                    &status_display,
                    &body,
                    request_id.as_deref(),
                    debug,
                )));
            }
            Err(e) => return Err(api_error(action, debug, e).await),
        }
    }
}

/// Re-run an operation lookup, re-prompting for the operation ID on recoverable errors.
#[allow(clippy::print_stderr)]
pub(crate) async fn fetch_with_operation_retry<T, F, Fut>(
    ctx: &CommandContext,
    initial_id: String,
    prompt: &str,
    action: &str,
    debug: bool,
    fetch: F,
) -> Result<T>
where
    F: Fn(domains_client::Client, String) -> Fut,
    Fut: Future<Output = std::result::Result<T, domains_client::Error<()>>>,
{
    let client = make_client(ctx).await?;
    let mut operation_id = initial_id;
    loop {
        match fetch(client.clone(), operation_id.clone()).await {
            Ok(value) => return Ok(value),
            Err(domains_client::Error::UnexpectedResponse(resp)) if ctx.is_interactive() => {
                let status = resp.status().as_u16();
                let status_display = resp.status().to_string();
                let request_id = resp
                    .headers()
                    .get("x-request-id")
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_owned);
                let body = resp.text().await.unwrap_or_default();
                if let Some(hint) = recoverable_operation_api_error(status, &body) {
                    eprintln!("  {hint}");
                    operation_id = prompt_validated_operation_id(prompt)?;
                    continue;
                }
                return Err(CliCoreError::message(format_api_error(
                    action,
                    status,
                    &status_display,
                    &body,
                    request_id.as_deref(),
                    debug,
                )));
            }
            Err(e) => return Err(api_error(action, debug, e).await),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recoverable_tld_api_error_matches_unsupported_tld() {
        let hint = recoverable_tld_api_error(
            422,
            r#"{"code":"UNSUPPORTED_TLD","message":"The specified TLD is currently unsupported"}"#,
        )
        .expect("recoverable");
        assert!(hint.contains("not supported"));
    }

    #[test]
    fn recoverable_tld_api_error_ignores_other_codes() {
        assert!(recoverable_tld_api_error(422, r#"{"code":"OTHER","message":"nope"}"#).is_none());
        assert!(recoverable_tld_api_error(404, r#"{"code":"UNSUPPORTED_TLD"}"#).is_none());
    }
}
