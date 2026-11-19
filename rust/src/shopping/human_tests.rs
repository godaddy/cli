use super::*;

#[test]
fn typed_checkout_responses_keep_payment_descriptions_in_json_and_human_views() {
    let payload = json!({
        "id": "checkout-1",
        "status": "ready_for_complete",
        "payment": {"instruments": [
            {"id": "wallet-1", "selected": true,
             "rich_text_description": "E_WALLET/PAYPAL PayPal Agreement example"},
            {"id": "card-1", "rich_text_description": "CREDIT_CARD/VISA 4242",
             "brand": "visa", "last_digits": "4242", "expiry_month": 12, "expiry_year": 2030}
        ]}
    });
    macro_rules! round_trip {
        ($response:ty) => {{
            let typed: $response =
                serde_json::from_value(payload.clone()).expect("decode response");
            serde_json::to_value(typed).expect("encode response")
        }};
    }
    for response in [
        round_trip!(shopping_client::types::CreateCheckoutResponse),
        round_trip!(shopping_client::types::GetCheckoutResponse),
        round_trip!(shopping_client::types::UpdateCheckoutResponse),
        round_trip!(shopping_client::types::CompleteCheckoutResponse),
    ] {
        let instruments = &response["payment"]["instruments"];
        for index in 0..2 {
            assert_eq!(
                instruments[index]["rich_text_description"],
                payload["payment"]["instruments"][index]["rich_text_description"]
            );
        }
        for field in ["brand", "last_digits", "expiry_month", "expiry_year"] {
            assert!(instruments[1].get(field).is_none(), "unexpected {field}");
        }
        let cart = checkout_response(&response, false);
        let human = render_checkout(&cart);
        assert!(
            human.contains(
                "Selected payment: E_WALLET/PAYPAL PayPal Agreement example (ID: wallet-1)"
            )
        );
        assert!(human.contains("- CREDIT_CARD/VISA 4242 (ID: card-1)"));
        let completion = checkout_completion_response(&response);
        let human = render_checkout_completion(&completion);
        assert!(
            human.contains(
                "Selected payment: E_WALLET/PAYPAL PayPal Agreement example (ID: wallet-1)"
            )
        );
        assert!(!human.contains("CREDIT_CARD/VISA"));
    }
}

#[test]
fn completion_payment_summary_handles_missing_description_and_selection() {
    let response = checkout_completion_response(&json!({
        "payment": {"instruments": [{"id": "card-1", "selected": true}]}
    }));
    assert!(
        render_checkout_completion(&response)
            .contains("Selected payment: Saved payment method (ID: card-1)")
    );
    for payment in [Value::Null, json!({"instruments": [{"id": "card-1"}]})] {
        let response = checkout_completion_response(&json!({"payment": payment}));
        assert!(!render_checkout_completion(&response).contains("Selected payment:"));
    }
}

fn catalog_response() -> Value {
    json!({
        "pagination": {"total_count": 1},
        "products": [{
            "id": "product-1",
            "title": "Product",
            "categories": [{"value": "email"}],
            "tags": ["Mailbox: 50 GB", "Apps: Office web apps"],
            "variants": [{
                "id": "product-1:1yr",
                "title": "One year",
                "description": {"plain": "Annual email plan."},
                "options": [{"name": "Term", "label": "1 year"}, {"name": "Mailbox", "label": "50 GB"}],
                "tags": ["Includes Office web apps"],
                "availability": {"available": true},
                "price": {"amount": 7188, "currency": "USD"},
                "renewal_price": {"amount": 11988, "currency": "USD"},
                "list_price": {"amount": 11988, "currency": "USD"}
            }]
        }]
    })
}

#[test]
fn catalog_search_groups_variants_without_optional_details() {
    let response = catalog_search_response(&catalog_response());
    let output = render_catalog_search(&response);

    assert_eq!(response["products"].as_array().map(Vec::len), Some(1));
    assert!(output.contains("Showing 1 of 1 products"));
    assert!(output.contains("1. Product (ID: product-1)"));
    assert!(output.contains("- One year"));
    assert!(output.contains("ID: product-1:1yr"));
    assert!(output.contains("Your price: USD 71.88"));
    assert!(output.contains("List price: USD 119.88"));
    assert!(!output.contains("Term:"));
    assert!(!output.contains("Availability:"));
}

#[test]
fn catalog_detail_reuses_projection_without_ucp_metadata() {
    let response = json!({"product": catalog_response()["products"][0].clone(), "ucp": {"do_not_render": true}});
    let output = render_catalog_product(&catalog_product_response(&response));

    assert!(output.contains("Your price: USD 71.88"));
    assert!(output.contains("Renews: USD 119.88"));
    assert!(output.contains("Term: 1 year · Mailbox: 50 GB"));
    assert!(output.contains("Annual email plan."));
    assert!(output.contains("Highlights: Includes Office web apps"));
    assert!(!output.contains("do_not_render"));
}

#[test]
fn catalog_lookup_uses_the_detailed_variant_renderer() {
    let output = render_catalog_products(&catalog_lookup_response(&catalog_response()));

    assert!(output.contains("Annual email plan."));
    assert!(output.contains("Term: 1 year · Mailbox: 50 GB"));
    assert!(output.contains("Highlights: Includes Office web apps"));
}

#[test]
fn checkout_response_and_human_view_include_required_agreements() {
    let response = checkout_response(
        &json!({
            "id": "checkout-1",
            "status": "ready_for_complete",
            "required_agreements": [{
                "key": "terms",
                "title": "Terms of Service",
                "url": "https://example.test/terms",
                "required": true
            }]
        }),
        false,
    );
    let output = render_checkout(&response);

    assert_eq!(response["required_agreements"][0]["key"], "terms");
    assert!(output.contains("Required agreements:"));
    assert!(output.contains("Terms of Service (terms): https://example.test/terms (Required)"));
    assert!(output.contains("Before completing checkout, please review every required agreement"));
    assert!(output.contains(
        "--agree flag on checkout completion acknowledges and accepts all required agreements"
    ));
    // The human-only view never carries the AI-assistant note (that's for
    // an assistant reading `--output json` on the user's behalf) — a human
    // reading their own terminal doesn't need to be told to confirm the
    // price with themselves.
    assert!(!output.contains("AI assistants:"));
}

#[test]
fn completed_checkout_omits_only_pre_completion_guidance() {
    let response = checkout_response(&json!({ "id": "checkout-1", "status": "completed" }), false);
    let output = render_checkout(&response);

    assert!(!output.contains("Before completing checkout"));
    assert!(!output.contains("AI assistants:"));
}

#[test]
fn empty_acknowledgement_renders_its_message_instead_of_a_blank_checkout() {
    let response = empty_acknowledgement_response("no data yet");

    assert_eq!(render_checkout(&response), "no data yet\n");
    assert_eq!(render_checkout_completion(&response), "no data yet\n");
    // A blank checkout/completion summary must never be mistaken for
    // this distinct "nothing came back" case.
    assert_ne!(render_checkout(&response), render_checkout(&Value::Null));
    assert_ne!(
        render_checkout_completion(&response),
        render_checkout_completion(&Value::Null)
    );
}

#[test]
fn catalog_search_shows_whether_each_purchase_option_is_available() {
    let mut response = catalog_response();
    response["products"][0]["variants"]
        .as_array_mut()
        .expect("variants")
        .push(json!({
            "id": "taken.com",
            "title": "taken.com",
            "availability": {"available": false}
        }));
    let output = render_catalog_search(&catalog_search_response(&response));

    assert!(output.contains("ID: product-1:1yr\n  Available: Yes\n"));
    assert!(output.contains("ID: taken.com\n  Available: No\n"));
}

#[test]
fn checkout_items_show_the_domain_name_and_term() {
    let cart = checkout_response(
        &json!({
            "id": "checkout-1",
            "status": "ready_for_complete",
            "line_items": [
                {"quantity": 1, "item": {"id": "domain", "title": ".COM Domain Registration",
                    "common_name": "example.com", "term": {"term_type": "YEAR", "number_of_terms": 2}}},
                {"quantity": 1, "item": {"id": "domain", "title": ".DE Domain Registration",
                    "common_name": "xn--bcher-kva.de", "display_name": "bücher.de",
                    "term": {"term_type": "YEAR", "number_of_terms": 1}}},
                {"quantity": 2, "item": {"id": "wsb-tier1", "title": "Website Builder",
                    "term": {"term_type": "MONTH", "number_of_terms": 1}}}
            ]
        }),
        false,
    );
    let output = render_checkout(&cart);

    assert_eq!(cart["items"][0]["domain"], "example.com");
    assert_eq!(cart["items"][0]["term"], "2 years");
    assert_eq!(cart["items"][1]["domain"], "bücher.de");
    assert_eq!(cart["items"][2]["domain"], Value::Null);
    assert!(
        output.contains("- 1 × .COM Domain Registration\n  Domain: example.com\n  Term: 2 years\n")
    );
    assert!(output.contains("  Domain: bücher.de\n  Term: 1 year\n"));
    assert!(output.contains("- 2 × Website Builder\n  Term: 1 month\n"));
}
