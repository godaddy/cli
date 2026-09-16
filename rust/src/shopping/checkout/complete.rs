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

    /// Acknowledge the checkout session's terms and other important links.
    #[arg(long)]
    agree: bool,

    /// Billing address as a JSON object. Supported fields: first_name, last_name, phone_number, street_address, extended_address, address_locality, address_region, postal_code, address_country.
    #[arg(long, value_name = "JSON")]
    billing_address: Option<String>,
}

fn completion_error(error: ClientError) -> cli_engine::CliCoreError {
    crate::error::GddyError::from(error).into_cli_error()
}

fn public_completion_body(body: &Value) -> Value {
    let mut body = body.clone();
    body.as_object_mut()
        .expect("completion body is an object")
        .remove("idempotency_key");
    body
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

fn agreement_gate(agree: bool) -> cli_engine::Result<()> {
    if agree {
        return Ok(());
    }
    Err(crate::error::GddyError::validation(
        "placing an order requires acknowledging the checkout session's terms and important links",
    )
    .with_fix(
        "Review the checkout session with `shopping checkout get <checkout-id>`, then re-run with --agree.",
    )
    .into_cli_error())
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_typed_with_context::<Args, _, _, _>(
        CommandSpec::from_args::<Args>("complete", "Place an order from a checkout session")
            .with_long(
                "Place an order with the checkout session's selected saved payment method, or use \
                 --payment-instrument to select one. Review the checkout session and its links first, \
                 then use --agree to acknowledge them.",
            )
            .with_system("shopping")
            .with_tier(Tier::Mutate)
            .mutates(true)
            .handles_dry_run(true)
            .with_scopes(SHOPPING_SCOPES)
            .auth_optional()
            .with_view_id(CHECKOUT_COMPLETE_VIEW_ID),
        |ctx, args: Args| async move {
            agreement_gate(args.agree)?;
            let billing_address = billing_address(args.billing_address.as_deref())?;
            let mut body = if let Some(payment_instrument) = args.payment_instrument {
                CheckoutInput {
                    payment_instrument: Some(payment_instrument),
                    ..CheckoutInput::default()
                }
                .completion_body()?
            } else {
                let checkout = make_client(&ctx)
                    .await?
                    .get_checkout(&args.id)
                    .await
                    .map_err(client_err)?;
                selected_payment_body(&checkout)?
            };
            if let Some(billing_address) = billing_address {
                body["payment"]["instruments"][0]["billing_address"] = billing_address;
            }
            require_selected_payment_instrument(&body)?;
            let idempotency_key = uuid::Uuid::new_v4().to_string();
            body.as_object_mut()
                .expect("completion body is an object")
                .insert(
                    "idempotency_key".to_owned(),
                    Value::String(idempotency_key.clone()),
                );
            if ctx.dry_run() {
                return Ok(CommandResult::new(json!({
                    "action": "dry-run: would place order",
                    "id": args.id,
                    "body": public_completion_body(&body),
                }))
                .with_dry_run());
            }

            let client = make_client(&ctx).await?;
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
        let error = agreement_gate(false).expect_err("must require --agree");
        assert!(error.to_string().contains("acknowledging"));
        assert!(agreement_gate(true).is_ok());
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
