use cli_engine::{CliCoreError, CommandContext, Result};
use shopping_client::types::{
    Buyer, Checkout, CheckoutCompleteRequest, CheckoutCompleteRequestSchema,
    CheckoutWritableRequest, CheckoutWritableRequestSchema, Context, Item, LineItem, Message,
    Payment, PaymentInstrumentSelectedPaymentInstrument, UcpRefsSchemaAttribution,
    UcpRefsSchemaBuyer, UcpRefsSchemaContext, UcpRefsSchemaFulfillment, UcpRefsSchemaLineItem,
    UcpRefsSchemaPayment, UcpRefsSchemaSignals,
};

use crate::error::GddyError;
use crate::next_action::next_action;
use crate::shopping::SHOPPING_SCOPES;
use crate::shopping::client::{ClientError, build_client};

pub(crate) async fn make_client(ctx: &CommandContext) -> Result<shopping_client::Client> {
    let required: Vec<String> = SHOPPING_SCOPES
        .iter()
        .map(|scope| (*scope).to_owned())
        .collect();
    let token = ctx.credential_with_scopes(&required).await?.token;
    let base_url = crate::environments::resolve(&ctx.middleware.env)?.api_url;
    build_client(base_url, token).map_err(client_err)
}

pub(crate) fn client_err(error: ClientError) -> CliCoreError {
    GddyError::from(error).into_cli_error()
}

/// Extracts a message's `type`/`content` regardless of which untagged
/// `Message` variant deserialization happened to pick — the generated
/// `Error`/`Warning`/`Info` variants all have entirely optional fields, so
/// they don't reliably discriminate by shape alone.
fn error_message_content(message: &Message) -> Option<&str> {
    let (type_, content) = match message {
        Message::Error(message) => (&message.type_, &message.content),
        Message::Warning(message) => (&message.type_, &message.content),
        Message::Info(message) => (&message.type_, &message.content),
    };
    (type_.as_deref() == Some("error"))
        .then_some(content.as_deref())
        .flatten()
}

fn collect_error_messages(messages: &[Message]) -> Vec<&str> {
    messages.iter().filter_map(error_message_content).collect()
}

/// `error_response` (the `oneOf` error variant every generated response
/// enum carries) has no properties, and none of `Checkout`/`Order`/the
/// catalog response types have any required fields — so untagged
/// deserialization always matches the success variant first, even for a
/// genuine API error payload, and the `ErrorResponse` match arms scattered
/// through `client.rs`/the command handlers are defense-in-depth for a
/// non-object payload, not this API's real error channel. In practice this
/// contract signals an in-band failure via an error-severity entry in an
/// otherwise-2xx response's `messages` array — this is the actual check
/// that catches it.
pub(crate) fn reject_response_errors(messages: &[Message]) -> Result<()> {
    let errors = collect_error_messages(messages);
    if errors.is_empty() {
        Ok(())
    } else {
        Err(GddyError::validation(errors.join(" ")).into_cli_error())
    }
}

/// Checkout-specific wrapper with a remediation hint relevant to a checkout
/// mutation — create/update/complete all share this, since a rejected
/// write is fixed the same way regardless of which one produced it.
pub(crate) fn update_response(checkout: Checkout) -> Result<Checkout> {
    let errors = collect_error_messages(&checkout.messages);
    if errors.is_empty() {
        Ok(checkout)
    } else {
        Err(GddyError::validation(errors.join(" "))
            .with_fix(
                "Review the checkout session and update its items, buyer details, currency, or selected payment method before trying again.",
            )
            .into_cli_error())
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct CheckoutInput {
    pub(crate) items: Vec<String>,
    pub(crate) currency: Option<String>,
    pub(crate) buyer_first_name: Option<String>,
    pub(crate) buyer_last_name: Option<String>,
    pub(crate) buyer_email: Option<String>,
    pub(crate) buyer_phone: Option<String>,
    pub(crate) payment_instrument: Option<String>,
}

impl CheckoutInput {
    pub(crate) fn create_body(&self) -> Result<CheckoutWritableRequest> {
        if self.items.is_empty() {
            return Err(
                GddyError::validation("checkout create requires at least one --item")
                    .into_cli_error(),
            );
        }
        self.body()
    }

    pub(crate) fn update_body(&self, checkout: &Checkout) -> Result<CheckoutWritableRequest> {
        if !self.has_update() {
            return Err(GddyError::validation(
                "checkout update requires at least one item, buyer, currency, or payment-method change",
            )
            .into_cli_error());
        }

        let mut body = checkout_to_writable(checkout)?;
        if !self.items.is_empty() {
            body.line_items = line_items(&self.items)?;
        }
        if let Some(currency) = &self.currency {
            let context = body
                .context
                .get_or_insert_with(|| UcpRefsSchemaContext(Default::default()));
            context.0.currency = Some(currency.to_owned());
        }
        if self.has_buyer_update() {
            let buyer = body
                .buyer
                .get_or_insert_with(|| UcpRefsSchemaBuyer(Default::default()));
            insert_optional_nonblank(
                &mut buyer.0.first_name,
                "first_name",
                self.buyer_first_name.as_deref(),
            )?;
            insert_optional_nonblank(
                &mut buyer.0.last_name,
                "last_name",
                self.buyer_last_name.as_deref(),
            )?;
            insert_optional_nonblank(&mut buyer.0.email, "email", self.buyer_email.as_deref())?;
            insert_optional_nonblank(
                &mut buyer.0.phone_number,
                "phone_number",
                self.buyer_phone.as_deref(),
            )?;
        }
        if let Some(payment_instrument) = &self.payment_instrument {
            body.payment = Some(payment_selection_body(nonblank_value(
                Some(payment_instrument),
                "--payment-instrument must be non-empty",
            )?)?);
        }
        Ok(CheckoutWritableRequest(body))
    }

    fn has_update(&self) -> bool {
        !self.items.is_empty()
            || self.currency.is_some()
            || self.has_buyer_update()
            || self.payment_instrument.is_some()
    }

    fn has_buyer_update(&self) -> bool {
        self.buyer_first_name.is_some()
            || self.buyer_last_name.is_some()
            || self.buyer_email.is_some()
            || self.buyer_phone.is_some()
    }

    pub(crate) fn completion_body(&self) -> Result<CheckoutCompleteRequest> {
        if !self.items.is_empty()
            || self.currency.is_some()
            || self.buyer_first_name.is_some()
            || self.buyer_last_name.is_some()
            || self.buyer_email.is_some()
            || self.buyer_phone.is_some()
        {
            return Err(GddyError::validation(
                "checkout complete only supports --payment-instrument in structured mode",
            )
            .into_cli_error());
        }
        let payment_instrument = nonblank_value(
            self.payment_instrument.as_deref(),
            "--payment-instrument must be non-empty",
        )?;
        Ok(CheckoutCompleteRequest(CheckoutCompleteRequestSchema {
            payment: Some(payment_selection_body(payment_instrument)?),
            ..Default::default()
        }))
    }

    fn body(&self) -> Result<CheckoutWritableRequest> {
        let mut body = CheckoutWritableRequestSchema::default();
        if !self.items.is_empty() {
            body.line_items = line_items(&self.items)?;
        }
        if let Some(currency) = &self.currency {
            body.context = Some(UcpRefsSchemaContext(Context {
                currency: Some(currency.to_owned()),
                ..Default::default()
            }));
        }
        let mut buyer = Buyer::default();
        insert_optional_nonblank(
            &mut buyer.first_name,
            "first_name",
            self.buyer_first_name.as_deref(),
        )?;
        insert_optional_nonblank(
            &mut buyer.last_name,
            "last_name",
            self.buyer_last_name.as_deref(),
        )?;
        insert_optional_nonblank(&mut buyer.email, "email", self.buyer_email.as_deref())?;
        insert_optional_nonblank(
            &mut buyer.phone_number,
            "phone_number",
            self.buyer_phone.as_deref(),
        )?;
        if self.has_buyer_update() {
            body.buyer = Some(UcpRefsSchemaBuyer(buyer));
        }
        if let Some(payment_instrument) = &self.payment_instrument {
            body.payment = Some(payment_selection_body(nonblank_value(
                Some(payment_instrument),
                "--payment-instrument must be non-empty",
            )?)?);
        }
        Ok(CheckoutWritableRequest(body))
    }
}

/// Projects a fetched `Checkout` into the shape the write endpoints expect.
/// The two are wire-compatible (each writable field wraps the exact same
/// inner type the read side uses, e.g. `UcpRefsSchemaBuyer(pub Buyer)`), so
/// this is a direct field mapping — no dynamic JSON involved.
///
/// `checkout.line_items` may legitimately be empty (e.g. every item was
/// removed) — `client::get_checkout` already rejects a malformed/empty API
/// response before a `Checkout` ever reaches here, so an empty cart at this
/// point is real, not a sign of missing data.
fn checkout_to_writable(checkout: &Checkout) -> Result<CheckoutWritableRequestSchema> {
    let line_items = checkout
        .line_items
        .iter()
        .map(writable_line_item)
        .collect::<Result<Vec<_>>>()?;
    let mut context = checkout.context.clone().unwrap_or_default();
    if context.currency.is_none()
        && let Some(currency) = checkout.currency.as_deref()
    {
        context.currency = Some(currency.to_owned());
    }
    let payment = match selected_payment(checkout) {
        Some(payment) => Some(preserved_payment_body(payment)?),
        None => None,
    };
    Ok(CheckoutWritableRequestSchema {
        attribution: checkout.attribution.clone().map(UcpRefsSchemaAttribution),
        buyer: checkout.buyer.clone().map(UcpRefsSchemaBuyer),
        context: Some(UcpRefsSchemaContext(context)),
        fulfillment: checkout.fulfillment.clone().map(UcpRefsSchemaFulfillment),
        line_items,
        payment,
        signals: checkout.signals.clone().map(UcpRefsSchemaSignals),
    })
}

fn writable_line_item(line_item: &LineItem) -> Result<UcpRefsSchemaLineItem> {
    let item_id = line_item
        .item
        .as_ref()
        .and_then(|item| item.id.clone())
        .ok_or_else(|| {
            GddyError::unexpected("checkout response contained a line item without an item ID")
                .into_cli_error()
        })?;
    let quantity = line_item.quantity.ok_or_else(|| {
        GddyError::unexpected("checkout response contained a line item without a quantity")
            .into_cli_error()
    })?;
    Ok(UcpRefsSchemaLineItem(LineItem {
        id: line_item.id.clone(),
        included_products: Vec::new(),
        input: line_item.input.clone(),
        item: Some(Item {
            id: Some(item_id),
            ..Default::default()
        }),
        parent_id: line_item.parent_id.clone(),
        quantity: Some(quantity),
        totals: Vec::new(),
    }))
}

fn selected_payment(checkout: &Checkout) -> Option<&PaymentInstrumentSelectedPaymentInstrument> {
    checkout.payment.as_ref().and_then(|payment| {
        payment
            .instruments
            .iter()
            .find(|instrument| instrument.selected == Some(true))
    })
}

pub(crate) fn selected_payment_id(checkout: &Checkout) -> Option<&str> {
    selected_payment(checkout).and_then(|payment| payment.id.as_deref())
}

pub(crate) fn payment_selection_body(instrument_id: &str) -> Result<UcpRefsSchemaPayment> {
    Ok(UcpRefsSchemaPayment(Payment {
        instruments: vec![PaymentInstrumentSelectedPaymentInstrument {
            id: Some(instrument_id.to_owned()),
            selected: Some(true),
            ..Default::default()
        }],
    }))
}

fn preserved_payment_body(
    payment: &PaymentInstrumentSelectedPaymentInstrument,
) -> Result<UcpRefsSchemaPayment> {
    let payment_id = payment.id.clone().ok_or_else(|| {
        GddyError::unexpected("checkout response contained a selected payment method without an ID")
            .into_cli_error()
    })?;
    Ok(UcpRefsSchemaPayment(Payment {
        instruments: vec![PaymentInstrumentSelectedPaymentInstrument {
            id: Some(payment_id),
            selected: Some(true),
            billing_address: payment.billing_address.clone(),
            // A payment handler can require these to route the instrument;
            // an unchanged checkout update must echo them back unchanged.
            handler_id: payment.handler_id.clone(),
            type_: payment.type_.clone(),
        }],
    }))
}

fn line_items(items: &[String]) -> Result<Vec<UcpRefsSchemaLineItem>> {
    items
        .iter()
        .map(|item| {
            let (id, quantity) = parse_item(item)?;
            Ok(UcpRefsSchemaLineItem(LineItem {
                item: Some(Item {
                    id: Some(id.to_owned()),
                    ..Default::default()
                }),
                quantity: Some(quantity),
                ..Default::default()
            }))
        })
        .collect()
}

fn parse_item(value: &str) -> Result<(&str, std::num::NonZeroU64)> {
    let value = value.trim();
    let (id, quantity) = match value.rsplit_once('=') {
        Some((id, quantity)) => {
            let quantity = quantity.parse::<u64>().map_err(|_| {
                GddyError::validation(format!(
                    "invalid --item {value:?}: quantity after '=' must be a positive integer"
                ))
                .into_cli_error()
            })?;
            (id, quantity)
        }
        None => (value, 1),
    };
    let quantity = std::num::NonZeroU64::new(quantity).ok_or_else(|| {
        GddyError::validation(format!(
            "invalid --item {value:?}: item ID must be non-empty and quantity must be positive"
        ))
        .into_cli_error()
    })?;
    if id.trim().is_empty() {
        return Err(GddyError::validation(format!(
            "invalid --item {value:?}: item ID must be non-empty and quantity must be positive"
        ))
        .into_cli_error());
    }
    Ok((id.trim(), quantity))
}

fn nonblank_value<'a>(value: Option<&'a str>, error: &str) -> Result<&'a str> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| GddyError::validation(error).into_cli_error())
}

fn insert_optional_nonblank(
    field: &mut Option<String>,
    key: &str,
    value: Option<&str>,
) -> Result<()> {
    if let Some(value) = value {
        *field = Some(
            nonblank_value(Some(value), &format!("--buyer-{key} must be non-empty"))?.to_owned(),
        );
    }
    Ok(())
}

pub(crate) fn merge_context_currency(context: &mut Option<Context>, currency: Option<&str>) {
    let Some(currency) = currency else {
        return;
    };
    context.get_or_insert_with(Default::default).currency = Some(currency.to_owned());
}

pub(crate) fn currency_code(value: &str) -> std::result::Result<String, String> {
    let normalized = value.trim().to_ascii_uppercase();
    iso_currency::Currency::from_code(&normalized)
        .is_some()
        .then_some(normalized)
        .ok_or_else(|| "currency must be a valid ISO 4217 code".to_owned())
}

pub(crate) fn reject_multiple_payment_instruments(
    instruments: &[PaymentInstrumentSelectedPaymentInstrument],
) -> Result<()> {
    if instruments.len() > 1 {
        return Err(GddyError::validation(
            "only one payment instrument may be specified for a checkout session",
        )
        .with_fix(
            "Specify one saved payment instrument, or omit payment until checkout completion.",
        )
        .into_cli_error());
    }
    Ok(())
}

pub(crate) fn require_selected_payment_instrument(
    instruments: &[PaymentInstrumentSelectedPaymentInstrument],
) -> Result<()> {
    reject_multiple_payment_instruments(instruments)?;
    if let [instrument] = instruments
        && instrument.selected == Some(true)
        && instrument
            .id
            .as_deref()
            .is_some_and(|id| !id.trim().is_empty())
    {
        Ok(())
    } else {
        Err(GddyError::validation(
            "checkout completion requires exactly one selected saved payment instrument with an ID",
        )
        .with_fix("Include payment.instruments with one saved instrument ID marked selected: true.")
        .into_cli_error())
    }
}

pub(crate) fn no_saved_payment_method_action(
    checkout: &Checkout,
    account_url: &str,
) -> Option<cli_engine::NextAction> {
    checkout
        .payment
        .as_ref()
        .is_some_and(|payment| payment.instruments.is_empty())
        .then(|| {
            next_action(
                "payment-methods add",
                format!(
                    "No saved payment method is available. Add one at {account_url}/payment-methods/add-payment, then retrieve this checkout session again."
                ),
            )
        })
}

#[cfg(test)]
mod tests {
    use shopping_client::types::{
        ComGodaddyShoppingInputSchemaInputValue, MessageError, MessageWarning,
    };

    use super::*;
    use crate::shopping::command_for_env;

    #[test]
    fn builds_structured_checkout_body() {
        let body = CheckoutInput {
            items: vec!["product-a=2".to_owned(), "product-b".to_owned()],
            currency: Some("GBP".to_owned()),
            buyer_first_name: Some("Jane".to_owned()),
            buyer_email: Some("jane@example.test".to_owned()),
            payment_instrument: Some("payment-1".to_owned()),
            ..CheckoutInput::default()
        }
        .create_body()
        .expect("structured input should build");

        assert_eq!(body.0.line_items.len(), 2);
        assert_eq!(
            body.0.line_items[0]
                .0
                .item
                .as_ref()
                .and_then(|item| item.id.clone()),
            Some("product-a".to_owned())
        );
        assert_eq!(body.0.line_items[0].0.quantity.map(|q| q.get()), Some(2));
        assert_eq!(body.0.line_items[1].0.quantity.map(|q| q.get()), Some(1));
        assert_eq!(
            body.0
                .context
                .as_ref()
                .and_then(|context| context.0.currency.clone()),
            Some("GBP".to_owned())
        );
        assert_eq!(
            body.0
                .buyer
                .as_ref()
                .and_then(|buyer| buyer.0.first_name.clone()),
            Some("Jane".to_owned())
        );
        assert_eq!(
            body.0
                .buyer
                .as_ref()
                .and_then(|buyer| buyer.0.email.clone()),
            Some("jane@example.test".to_owned())
        );
        let instruments = &body.0.payment.expect("payment").0.instruments;
        assert_eq!(instruments.len(), 1);
        assert_eq!(instruments[0].id, Some("payment-1".to_owned()));
        assert_eq!(instruments[0].selected, Some(true));
    }

    #[test]
    fn rejects_invalid_structured_checkout_items() {
        assert!(
            CheckoutInput {
                items: vec!["product=0".to_owned()],
                ..CheckoutInput::default()
            }
            .create_body()
            .is_err()
        );
        assert!(
            CheckoutInput {
                items: vec!["product=two".to_owned()],
                ..CheckoutInput::default()
            }
            .create_body()
            .is_err()
        );
    }

    fn checkout() -> Checkout {
        Checkout {
            line_items: vec![LineItem {
                id: Some("line-1".to_owned()),
                item: Some(Item {
                    id: Some("product-1".to_owned()),
                    title: Some("Product".to_owned()),
                    ..Default::default()
                }),
                quantity: std::num::NonZeroU64::new(2),
                input: Some(ComGodaddyShoppingInputSchemaInputValue(
                    serde_json::json!({"domain": "example.com"}),
                )),
                included_products: vec![],
                totals: vec![],
                parent_id: None,
            }],
            context: Some(Context {
                currency: Some("USD".to_owned()),
                language: Some("en".to_owned()),
                ..Default::default()
            }),
            buyer: Some(Buyer {
                first_name: Some("Jane".to_owned()),
                last_name: Some("Doe".to_owned()),
                email: Some("jane@example.test".to_owned()),
                phone_number: None,
            }),
            payment: Some(Payment {
                instruments: vec![PaymentInstrumentSelectedPaymentInstrument {
                    id: Some("payment-1".to_owned()),
                    selected: Some(true),
                    handler_id: Some("com.godaddy.payments".to_owned()),
                    type_: Some("card".to_owned()),
                    ..Default::default()
                }],
            }),
            ..Default::default()
        }
    }

    #[test]
    fn update_preserves_the_selected_instrument_handler_routing_metadata_when_unchanged() {
        let body = CheckoutInput {
            buyer_email: Some("updated@example.test".to_owned()),
            ..CheckoutInput::default()
        }
        .update_body(&checkout())
        .expect("buyer-only update should be valid");

        let instrument = &body.0.payment.expect("payment").0.instruments[0];
        assert_eq!(instrument.id, Some("payment-1".to_owned()));
        assert_eq!(
            instrument.handler_id,
            Some("com.godaddy.payments".to_owned()),
            "an unchanged selected instrument must keep its handler routing metadata"
        );
        assert_eq!(instrument.type_, Some("card".to_owned()));
    }

    #[test]
    fn update_rebuilds_full_writable_state_and_merges_explicit_changes() {
        let body = CheckoutInput {
            buyer_email: Some("updated@example.test".to_owned()),
            payment_instrument: Some("payment-2".to_owned()),
            ..CheckoutInput::default()
        }
        .update_body(&checkout())
        .expect("buyer and payment updates should be valid");

        assert_eq!(body.0.line_items.len(), 1);
        let line_item = &body.0.line_items[0].0;
        assert_eq!(line_item.id, Some("line-1".to_owned()));
        assert_eq!(
            line_item.item.as_ref().and_then(|item| item.id.clone()),
            Some("product-1".to_owned())
        );
        assert!(
            line_item
                .item
                .as_ref()
                .is_some_and(|item| item.title.is_none())
        );
        assert!(line_item.totals.is_empty());
        assert_eq!(
            body.0
                .context
                .as_ref()
                .and_then(|context| context.0.currency.clone()),
            Some("USD".to_owned())
        );
        let buyer = body.0.buyer.expect("buyer").0;
        assert_eq!(buyer.first_name, Some("Jane".to_owned()));
        assert_eq!(buyer.last_name, Some("Doe".to_owned()));
        assert_eq!(buyer.email, Some("updated@example.test".to_owned()));
        let instruments = body.0.payment.expect("payment").0.instruments;
        assert_eq!(instruments[0].id, Some("payment-2".to_owned()));
    }

    #[test]
    fn currency_update_preserves_state_and_uses_explicit_payment_selection() {
        let body = CheckoutInput {
            currency: Some("GBP".to_owned()),
            payment_instrument: Some("payment-2".to_owned()),
            ..CheckoutInput::default()
        }
        .update_body(&checkout())
        .expect("currency change with explicit payment should be valid");

        assert_eq!(
            body.0
                .context
                .as_ref()
                .and_then(|context| context.0.currency.clone()),
            Some("GBP".to_owned())
        );
        assert_eq!(
            body.0.payment.as_ref().expect("payment").0.instruments[0].id,
            Some("payment-2".to_owned())
        );
        assert_eq!(
            body.0.line_items[0]
                .0
                .item
                .as_ref()
                .and_then(|item| item.id.clone()),
            Some("product-1".to_owned())
        );
    }

    #[test]
    fn update_replaces_items_and_retains_other_writable_state() {
        let body = CheckoutInput {
            items: vec!["product-2=3".to_owned()],
            ..CheckoutInput::default()
        }
        .update_body(&checkout())
        .expect("item replacement should be valid");
        assert_eq!(body.0.line_items.len(), 1);
        assert_eq!(
            body.0.line_items[0]
                .0
                .item
                .as_ref()
                .and_then(|item| item.id.clone()),
            Some("product-2".to_owned())
        );
        assert_eq!(body.0.line_items[0].0.quantity.map(|q| q.get()), Some(3));
        assert_eq!(
            body.0
                .buyer
                .as_ref()
                .and_then(|buyer| buyer.0.email.clone()),
            Some("jane@example.test".to_owned())
        );
        assert_eq!(
            body.0.payment.expect("payment").0.instruments[0].id,
            Some("payment-1".to_owned())
        );
    }

    #[test]
    fn update_rejects_when_no_changes_are_requested() {
        assert!(CheckoutInput::default().update_body(&checkout()).is_err());
    }

    #[test]
    fn update_allows_a_legitimately_empty_cart() {
        // `client::get_checkout` already rejects a malformed/empty API
        // response before a real caller ever reaches `update_body`, so an
        // empty `line_items` here represents a checkout whose items were
        // all removed, not missing data — it must still be updatable.
        let empty_cart = Checkout {
            id: Some("checkout-1".to_owned()),
            line_items: vec![],
            ..checkout()
        };
        let body = CheckoutInput {
            buyer_email: Some("jane@example.test".to_owned()),
            ..CheckoutInput::default()
        }
        .update_body(&empty_cart)
        .expect("an empty cart should still accept a buyer update");

        assert!(body.0.line_items.is_empty());
        assert_eq!(
            body.0
                .buyer
                .as_ref()
                .and_then(|buyer| buyer.0.email.clone()),
            Some("jane@example.test".to_owned())
        );
    }

    #[test]
    fn builds_structured_completion_with_one_payment_instrument() {
        let body = CheckoutInput {
            payment_instrument: Some("payment-1".to_owned()),
            ..CheckoutInput::default()
        }
        .completion_body()
        .expect("completion body should build");

        let instruments = body.0.payment.expect("payment").0.instruments;
        assert_eq!(instruments[0].id, Some("payment-1".to_owned()));
        assert_eq!(instruments[0].selected, Some(true));
    }

    #[test]
    fn adds_payment_method_actions_for_resolved_environment_urls() {
        let empty_instruments = Checkout {
            payment: Some(Payment {
                instruments: vec![],
            }),
            ..Default::default()
        };
        for account_url in [
            "https://account.godaddy.com",
            "https://account.test-godaddy.com",
            "https://account.dev-godaddy.com",
        ] {
            let action = no_saved_payment_method_action(&empty_instruments, account_url)
                .expect("empty list should require a payment method");
            assert_eq!(action.command, "gddy payment-methods add");
            assert!(
                action
                    .description
                    .contains(&format!("{account_url}/payment-methods/add-payment"))
            );
        }
        assert!(
            no_saved_payment_method_action(
                &Checkout {
                    payment: Some(Payment::default()),
                    ..Default::default()
                },
                "https://account.godaddy.com"
            )
            .is_some()
        );
        assert!(
            no_saved_payment_method_action(&Checkout::default(), "https://account.godaddy.com")
                .is_none()
        );
    }

    #[test]
    fn reports_api_update_errors() {
        let checkout = Checkout {
            messages: vec![Message::Error(MessageError {
                content: Some("invalid payment".to_owned()),
                type_: Some("error".to_owned()),
                ..Default::default()
            })],
            ..Default::default()
        };
        assert!(
            update_response(checkout)
                .expect_err("API error should be reported")
                .to_string()
                .contains("invalid payment")
        );
    }

    #[test]
    fn reject_response_errors_surfaces_error_severity_messages_only() {
        let messages = vec![
            Message::Warning(MessageWarning {
                content: Some("a warning, not an error".to_owned()),
                type_: Some("warning".to_owned()),
                ..Default::default()
            }),
            Message::Error(MessageError {
                content: Some("catalog search failed upstream".to_owned()),
                type_: Some("error".to_owned()),
                ..Default::default()
            }),
        ];

        let error = reject_response_errors(&messages)
            .expect_err("an error-severity message must be reported");
        assert!(error.to_string().contains("catalog search failed upstream"));
        assert!(!error.to_string().contains("a warning, not an error"));

        assert!(reject_response_errors(&[]).is_ok());
    }

    #[test]
    fn requires_exactly_one_payment_instrument() {
        assert!(
            require_selected_payment_instrument(&[PaymentInstrumentSelectedPaymentInstrument {
                id: Some("payment-1".to_owned()),
                selected: Some(true),
                ..Default::default()
            }])
            .is_ok()
        );
        assert!(require_selected_payment_instrument(&[]).is_err());
        assert!(
            require_selected_payment_instrument(&[
                PaymentInstrumentSelectedPaymentInstrument {
                    id: Some("payment-1".to_owned()),
                    selected: Some(true),
                    ..Default::default()
                },
                PaymentInstrumentSelectedPaymentInstrument {
                    id: Some("payment-2".to_owned()),
                    selected: Some(false),
                    ..Default::default()
                },
            ])
            .is_err()
        );
    }

    #[test]
    fn follow_up_commands_rely_on_the_selected_environment() {
        assert_eq!(
            command_for_env("test", "order get order-1 --wait"),
            "shopping order get order-1 --wait"
        );
    }
}
