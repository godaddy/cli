use std::collections::BTreeMap;

use cli_engine::{CommandResult, CommandSpec, NextActionParam, RuntimeCommandSpec, Tier};
use serde_json::{Value, json};

use crate::next_action::next_action;
use crate::shopping::SHOPPING_SCOPES;
use crate::shopping::client::ClientError;
use crate::shopping::common::{
    CheckoutInput, client_err, make_client, require_selected_payment_instrument,
};
use crate::shopping::human::{
    CHECKOUT_COMPLETE_VIEW_ID, checkout_completion_response, selected_payment_id,
};

#[derive(Debug, Clone, clap::Args)]
struct Args {
    /// Checkout session ID.
    #[arg(value_name = "CHECKOUT_ID")]
    id: String,

    /// Saved payment method ID to use for this purchase. Omit to use the currently selected method.
    #[arg(long, value_name = "INSTRUMENT_ID")]
    payment_instrument: Option<String>,

    /// Accept every required agreement shown for the checkout session.
    #[arg(long)]
    agree: bool,

    /// Billing address as a JSON object. Supported fields: first_name, last_name, phone_number, street_address, extended_address, address_locality, address_region, postal_code, address_country.
    #[arg(long, value_name = "JSON")]
    billing_address: Option<String>,
}

fn completion_error(error: ClientError) -> cli_engine::CliCoreError {
    crate::error::GddyError::from(error).into_cli_error()
}

fn selected_payment_body(checkout: &Value) -> cli_engine::Result<Value> {
    let payment_instrument = selected_payment_id(checkout).ok_or_else(|| {
        crate::error::GddyError::validation("no payment method is selected for this checkout session")
            .with_fix(
                "Select one with `shopping checkout update <checkout-id> --payment-instrument <payment-instrument-id>`.",
            )
            .into_cli_error()
    })?;
    Ok(json!({"payment": {"instruments": [{"id": payment_instrument, "selected": true}]}}))
}

const BILLING_ADDRESS_FIELDS: &[&str] = &[
    "first_name",
    "last_name",
    "phone_number",
    "street_address",
    "extended_address",
    "address_locality",
    "address_region",
    "postal_code",
    "address_country",
];

fn billing_address(input: Option<&str>) -> cli_engine::Result<Option<Value>> {
    let Some(input) = input else {
        return Ok(None);
    };
    let address: Value = serde_json::from_str(input).map_err(|error| {
        crate::error::GddyError::validation(format!("invalid --billing-address JSON: {error}"))
            .into_cli_error()
    })?;
    let address = address.as_object().ok_or_else(|| {
        crate::error::GddyError::validation("--billing-address must be a JSON object")
            .into_cli_error()
    })?;
    if address.is_empty() {
        return Err(crate::error::GddyError::validation(
            "--billing-address must contain at least one address field",
        )
        .into_cli_error());
    }
    for (field, value) in address {
        if !BILLING_ADDRESS_FIELDS.contains(&field.as_str()) {
            return Err(crate::error::GddyError::validation(format!(
                "--billing-address does not support field {field:?}"
            ))
            .with_fix(format!("Use only: {}.", BILLING_ADDRESS_FIELDS.join(", ")))
            .into_cli_error());
        }
        if !value.is_string() || value.as_str().is_none_or(|value| value.trim().is_empty()) {
            return Err(crate::error::GddyError::validation(format!(
                "--billing-address field {field:?} must be a non-empty string"
            ))
            .into_cli_error());
        }
    }
    Ok(Some(Value::Object(address.clone())))
}

fn required_agreements(checkout: &Value) -> Vec<(&str, &str, Option<&str>)> {
    checkout
        .get("required_agreements")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|agreement| agreement.get("required").and_then(Value::as_bool) == Some(true))
        .filter_map(|agreement| {
            let key = agreement.get("key").and_then(Value::as_str)?;
            if key.trim().is_empty() {
                return None;
            }
            let title = agreement
                .get("title")
                .and_then(Value::as_str)
                .filter(|title| !title.trim().is_empty())
                .unwrap_or(key);
            Some((key, title, agreement.get("url").and_then(Value::as_str)))
        })
        .fold(BTreeMap::new(), |mut agreements, (key, title, url)| {
            agreements.entry(key).or_insert((title, url));
            agreements
        })
        .into_iter()
        .map(|(key, (title, url))| (key, title, url))
        .collect()
}

fn agreement_gate(checkout: &Value, agree: bool) -> cli_engine::Result<()> {
    if agree {
        return Ok(());
    }
    let agreements = required_agreements(checkout);
    let details = agreements
        .iter()
        .map(|(key, title, url)| match url {
            Some(url) => format!("  - {title} ({key}): {url}"),
            None => format!("  - {title} ({key})"),
        })
        .collect::<Vec<_>>()
        .join("\n");
    let message = if details.is_empty() {
        "placing an order requires acknowledging the checkout session's terms and important links"
            .to_owned()
    } else {
        format!("placing an order requires accepting these checkout session agreements:\n{details}")
    };
    Err(crate::error::GddyError::validation(message)
        .with_fix(
            "Review the checkout session with `shopping checkout get <checkout-id>`, then re-run with --agree.",
        )
        .into_cli_error())
}

fn completion_consent(checkout: &Value) -> Option<Value> {
    let agreement_types = required_agreements(checkout)
        .into_iter()
        .map(|(key, _, _)| Value::String(key.to_owned()))
        .collect::<Vec<_>>();
    if agreement_types.is_empty() {
        return None;
    }
    Some(json!({
        "agreement_types": agreement_types,
        "agreed_at": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
    }))
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<Args, _, _, _>(
        CommandSpec::from_args::<Args>("complete", "Place an order from a checkout session")
            .with_long(
                "Place an order with the checkout session's selected saved payment method, or use \
                 --payment-instrument to select one. Review its required agreements and important links \
                 first, then use --agree to accept every required agreement.",
            )
            .with_system("shopping")
            .with_tier(Tier::Mutate)
            .mutates(true)
            .handles_dry_run(true)
            .with_scopes(SHOPPING_SCOPES)
            .auth_optional()
            .with_view_id(CHECKOUT_COMPLETE_VIEW_ID),
        |ctx, args: Args| async move {
            let client = make_client(&ctx).await?;
            let checkout = client
                .get_checkout(&args.id)
                .await
                .map_err(client_err)?;
            agreement_gate(&checkout, args.agree)?;
            let billing_address = billing_address(args.billing_address.as_deref())?;
            let mut body = if let Some(payment_instrument) = args.payment_instrument {
                CheckoutInput {
                    payment_instrument: Some(payment_instrument),
                    ..CheckoutInput::default()
                }
                .completion_body()?
            } else {
                selected_payment_body(&checkout)?
            };
            if let Some(consent) = completion_consent(&checkout) {
                body["consent"] = consent;
            }
            if let Some(billing_address) = billing_address {
                body["payment"]["instruments"][0]["billing_address"] = billing_address;
            }
            require_selected_payment_instrument(&body)?;
            let idempotency_key = uuid::Uuid::new_v4().to_string();
            if ctx.dry_run() {
                return Ok(CommandResult::new(json!({
                    "action": "dry-run: would place order",
                    "id": args.id,
                    "body": body,
                }))
                .with_dry_run());
            }

            let completion = client
                .complete_checkout(&args.id, body, &idempotency_key)
                .await
                .map_err(completion_error)?;
            let order_id = completion.pointer("/order/id").and_then(Value::as_str);
            let actions = order_id.map_or_else(Vec::new, |order_id| {
                vec![
                    next_action(
                        "shopping order get <order-id> --wait",
                        "Review the order after it becomes visible",
                    )
                    .with_param("order-id", NextActionParam::value(order_id)),
                ]
            });
            let output = if ctx.middleware.output_format == "human" {
                checkout_completion_response(&completion)
            } else {
                completion
            };
            Ok(CommandResult::new(output).with_next_actions(actions))
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agreement_gate_requires_agree() {
        let checkout = json!({
            "required_agreements": [{
                "key": "universal_terms_and_conditions",
                "title": "Universal Terms of Service Agreement",
                "url": "https://www.godaddy.com/legal/agreements/universal-terms-of-service-agreement",
                "required": true
            }]
        });
        let error = agreement_gate(&checkout, false).expect_err("must require --agree");
        assert!(
            error
                .to_string()
                .contains("Universal Terms of Service Agreement")
        );
        assert!(error.to_string().contains("universal_terms_and_conditions"));
        assert!(agreement_gate(&checkout, true).is_ok());
    }

    #[test]
    fn completion_consent_includes_every_required_agreement() {
        let checkout = json!({
            "required_agreements": [
                {"key": "terms", "required": true},
                {"key": "optional_marketing", "required": false},
                {"key": "ssl", "required": true}
            ]
        });
        let consent = completion_consent(&checkout).expect("required consent");

        assert_eq!(consent["agreement_types"], json!(["ssl", "terms"]));
        assert!(
            chrono::DateTime::parse_from_rfc3339(consent["agreed_at"].as_str().expect("timestamp"))
                .is_ok()
        );
    }

    #[test]
    fn completion_consent_is_omitted_without_required_agreements() {
        assert!(completion_consent(&json!({"required_agreements": []})).is_none());
    }

    #[test]
    fn validates_billing_address() {
        let address = billing_address(Some(
            r#"{"street_address":"123 Main St","address_locality":"Mountain View","address_country":"US"}"#,
        ))
        .expect("valid billing address")
        .expect("provided address");

        assert_eq!(address["address_locality"], "Mountain View");
        assert!(billing_address(Some("[]")).is_err());
        assert!(billing_address(Some(r#"{"city":"Mountain View"}"#)).is_err());
        assert!(billing_address(Some(r#"{"street_address":""}"#)).is_err());
    }

    #[test]
    fn builds_completion_body_from_currently_selected_payment_method() {
        let body = selected_payment_body(&json!({
            "payment": {"instruments": [
                {"id": "payment-1", "selected": false},
                {"id": "payment-2", "selected": true}
            ]}
        }))
        .expect("selected payment method should be used");

        assert_eq!(
            body,
            json!({"payment": {"instruments": [{"id": "payment-2", "selected": true}]}})
        );
    }

    #[test]
    fn selected_payment_method_is_required_when_completion_omits_one() {
        let error = selected_payment_body(&json!({"payment": {"instruments": []}}))
            .expect_err("a selected payment method is required");

        assert!(error.to_string().contains("no payment method is selected"));
    }

    #[test]
    fn human_response_shows_completion_total_only_with_currency() {
        let response = checkout_completion_response(&json!({
            "id": "checkout-1",
            "status": "completed",
            "currency": "GBP",
            "totals": [
                {"type": "subtotal", "amount": 4788},
                {"type": "tax", "amount": 0},
                {"type": "total", "amount": 4788}
            ]
        }));

        assert_eq!(response["total"], "GBP 47.88");
        assert!(!response.to_string().contains("subtotal"));
        assert!(!response.to_string().contains("tax"));
    }

    #[test]
    fn human_response_omits_completion_total_without_currency() {
        let response = checkout_completion_response(&json!({
            "id": "checkout-1",
            "status": "completed",
            "totals": [{"type": "total", "amount": 4788}]
        }));

        assert!(response.get("total").is_some_and(Value::is_null));
        assert!(!response.to_string().contains("4788"));
    }
}
