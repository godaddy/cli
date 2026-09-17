use std::collections::BTreeMap;

use cli_engine::{CommandResult, CommandSpec, NextActionParam, RuntimeCommandSpec, Tier};
use serde_json::{Map, Value};
use shopping_client::types::{
    CatalogLookupLookupRequest, Checkout, CheckoutCompleteRequest, CheckoutCompleteRequestSchema,
    LookupCatalogResponse, LookupRequest, ShoppingConsentAcceptance, UcpRefsSchemaPayment,
};

use crate::next_action::next_action;
use crate::shopping::SHOPPING_SCOPES;
use crate::shopping::client::ClientError;
use crate::shopping::common::{
    CheckoutInput, client_err, make_client, payment_selection_body,
    require_selected_payment_instrument, selected_payment_id,
};
use crate::shopping::human::{CHECKOUT_COMPLETE_VIEW_ID, checkout_completion_response};

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

fn selected_payment_body(checkout: &Checkout) -> cli_engine::Result<UcpRefsSchemaPayment> {
    let payment_instrument = selected_payment_id(checkout).ok_or_else(|| {
        crate::error::GddyError::validation("no payment method is selected for this checkout session")
            .with_fix(
                "Select one with `shopping checkout update <checkout-id> --payment-instrument <payment-instrument-id>`.",
            )
            .into_cli_error()
    })?;
    payment_selection_body(payment_instrument)
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

fn billing_address(input: Option<&str>) -> cli_engine::Result<Option<Map<String, Value>>> {
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
    Ok(Some(address.clone()))
}

fn required_agreements(checkout: &Checkout) -> Vec<(&str, &str, Option<&str>)> {
    checkout
        .required_agreements
        .iter()
        .filter(|agreement| agreement.required == Some(true))
        .filter_map(|agreement| {
            let key = agreement.key.as_deref()?;
            if key.trim().is_empty() {
                return None;
            }
            let title = agreement
                .title
                .as_deref()
                .filter(|title| !title.trim().is_empty())
                .unwrap_or(key);
            Some((key, title, agreement.url.as_deref()))
        })
        .fold(BTreeMap::new(), |mut agreements, (key, title, url)| {
            agreements.entry(key).or_insert((title, url));
            agreements
        })
        .into_iter()
        .map(|(key, (title, url))| (key, title, url))
        .collect()
}

fn agreement_gate(checkout: &Checkout, agree: bool) -> cli_engine::Result<()> {
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

fn completion_consent(checkout: &Checkout) -> Option<ShoppingConsentAcceptance> {
    let agreement_types = required_agreements(checkout)
        .into_iter()
        .map(|(key, _, _)| key.to_owned())
        .collect::<Vec<_>>();
    if agreement_types.is_empty() {
        return None;
    }
    Some(ShoppingConsentAcceptance {
        agreement_types,
        agreed_at: Some(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)),
    })
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
            let checkout = crate::shopping::client::get_checkout(&client, &args.id)
                .await
                .map_err(client_err)?;
            agreement_gate(&checkout, args.agree)?;
            let product_ids = crate::shopping::product_actions::purchased_product_ids(&checkout);
            let product_actions = if product_ids.is_empty() {
                Vec::new()
            } else {
                crate::shopping::client::decode::<LookupCatalogResponse>(
                    client
                        .lookup_catalog()
                        .body(LookupRequest(CatalogLookupLookupRequest {
                            ids: product_ids,
                            ..Default::default()
                        }))
                        .send()
                        .await,
                )
                .await
                .map(|response| match response {
                    Some(LookupCatalogResponse::LookupResponse(response)) => {
                        crate::shopping::product_actions::post_purchase_actions(&response)
                    }
                    _ => Vec::new(),
                })
                .unwrap_or_else(|error| {
                    tracing::warn!(error = %error, "could not look up purchased product categories");
                    Vec::new()
                })
            };
            let billing_address = billing_address(args.billing_address.as_deref())?;
            let mut body = if let Some(payment_instrument) = args.payment_instrument {
                CheckoutInput {
                    payment_instrument: Some(payment_instrument),
                    ..CheckoutInput::default()
                }
                .completion_body()?
            } else {
                CheckoutCompleteRequest(CheckoutCompleteRequestSchema {
                    payment: Some(selected_payment_body(&checkout)?),
                    ..Default::default()
                })
            };
            if let Some(consent) = completion_consent(&checkout) {
                body.0.consent = Some(consent);
            }
            if let Some(billing_address) = billing_address
                && let Some(payment) = body.0.payment.as_mut()
                && let Some(instrument) = payment.0.instruments.first_mut()
            {
                instrument.billing_address = billing_address;
            }
            let instruments = body
                .0
                .payment
                .as_ref()
                .map_or(&[][..], |payment| payment.0.instruments.as_slice());
            require_selected_payment_instrument(instruments)?;
            let idempotency_key = uuid::Uuid::new_v4().to_string();
            if ctx.dry_run() {
                return Ok(CommandResult::new(serde_json::json!({
                    "action": "dry-run: would place order",
                    "id": args.id,
                    "body": serde_json::to_value(&body).map_err(|error| {
                        crate::error::GddyError::unexpected(format!(
                            "failed to encode checkout completion request: {error}"
                        ))
                        .into_cli_error()
                    })?,
                }))
                .with_dry_run());
            }

            let completion = crate::shopping::client::complete_checkout(
                &client,
                &args.id,
                body,
                &idempotency_key,
            )
            .await
            .map_err(completion_error)?;
            let order_id = completion.order.as_ref().and_then(|order| order.id.clone());
            let mut actions = order_id.map_or_else(Vec::new, |order_id| {
                vec![
                    next_action(
                        "shopping order get <order-id> --wait",
                        "Review the order after it becomes visible",
                    )
                    .with_param("order-id", NextActionParam::value(order_id)),
                ]
            });
            actions.extend(product_actions);
            let completion = serde_json::to_value(&completion).map_err(|error| {
                crate::error::GddyError::unexpected(format!(
                    "failed to encode checkout completion response: {error}"
                ))
                .into_cli_error()
            })?;
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
    use serde_json::json;
    use shopping_client::types::{
        Payment, PaymentInstrumentSelectedPaymentInstrument, ShoppingRequiredAgreement,
    };

    use super::*;

    fn required_agreement(
        key: &str,
        title: Option<&str>,
        url: Option<&str>,
        required: bool,
    ) -> ShoppingRequiredAgreement {
        ShoppingRequiredAgreement {
            key: Some(key.to_owned()),
            title: title.map(str::to_owned),
            url: url.map(str::to_owned),
            required: Some(required),
            content: None,
        }
    }

    #[test]
    fn agreement_gate_requires_agree() {
        let checkout = Checkout {
            required_agreements: vec![required_agreement(
                "universal_terms_and_conditions",
                Some("Universal Terms of Service Agreement"),
                Some(
                    "https://www.godaddy.com/legal/agreements/universal-terms-of-service-agreement",
                ),
                true,
            )],
            ..Default::default()
        };
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
        let checkout = Checkout {
            required_agreements: vec![
                required_agreement("terms", None, None, true),
                required_agreement("optional_marketing", None, None, false),
                required_agreement("ssl", None, None, true),
            ],
            ..Default::default()
        };
        let consent = completion_consent(&checkout).expect("required consent");

        assert_eq!(
            consent.agreement_types,
            vec!["ssl".to_owned(), "terms".to_owned()]
        );
        assert!(
            chrono::DateTime::parse_from_rfc3339(consent.agreed_at.as_deref().expect("timestamp"))
                .is_ok()
        );
    }

    #[test]
    fn completion_consent_is_omitted_without_required_agreements() {
        assert!(completion_consent(&Checkout::default()).is_none());
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

    fn instrument(id: &str, selected: bool) -> PaymentInstrumentSelectedPaymentInstrument {
        PaymentInstrumentSelectedPaymentInstrument {
            id: Some(id.to_owned()),
            selected: Some(selected),
            ..Default::default()
        }
    }

    #[test]
    fn builds_completion_body_from_currently_selected_payment_method() {
        let checkout = Checkout {
            payment: Some(Payment {
                instruments: vec![
                    instrument("payment-1", false),
                    instrument("payment-2", true),
                ],
            }),
            ..Default::default()
        };
        let body =
            selected_payment_body(&checkout).expect("selected payment method should be used");

        assert_eq!(body.0.instruments.len(), 1);
        assert_eq!(body.0.instruments[0].id, Some("payment-2".to_owned()));
        assert_eq!(body.0.instruments[0].selected, Some(true));
    }

    #[test]
    fn selected_payment_method_is_required_when_completion_omits_one() {
        let checkout = Checkout {
            payment: Some(Payment {
                instruments: vec![],
            }),
            ..Default::default()
        };
        let error =
            selected_payment_body(&checkout).expect_err("a selected payment method is required");

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
