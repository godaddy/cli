//! `gddy domain suggest` — suggest available domains for a query (v3).

use cli_engine::{CommandResult, CommandSpec, NextActionParam, RuntimeCommandSpec, Tier};
use serde_json::json;

use domains_client::types;

use super::common::{
    api_error, comma_joined, format_money, make_client, string_list, term_for_period,
};
use crate::next_action::next_action;
use crate::output_schema::output_schema;
use crate::scopes::DOMAINS_READ;

// The handler emits a transformed per-item shape (formatted price/renewal-price
// strings per term, not the raw `types::Suggestion`), so declare the schema to
// match what's actually returned for `--schema`/help.
output_schema!(DomainSuggestResult {
    "domain": "string";
    // Present only when the API returns a formattable price for that term.
    "price1Year": "string", optional;
    "renewalPrice1Year": "string", optional;
    "price2Year": "string", optional;
    "renewalPrice2Year": "string", optional;
    "currency": "string", optional;
});

/// Convert a count-style flag value to the `NonZeroU64` the suggest query params
/// expect, or `None` when it's absent/zero/negative (so the param is simply
/// omitted). Used for `--limit`/`--length-min`/`--length-max`, which are
/// already clap-validated to be positive when present; total — never panics —
/// anyway, since `0`/negative simply means "unset".
fn nonzero(n: i64) -> Option<std::num::NonZeroU64> {
    u64::try_from(n).ok().and_then(std::num::NonZeroU64::new)
}

/// Build the JSON view of a single suggestion, flattening its 1- and 2-year
/// `TermPrice` entries into scalar price/renewal-price fields (v3 returns
/// indicative pricing as a `prices` array, which doesn't project into a table).
/// `None` when the suggestion has no domain, so every emitted object matches the
/// schema (`domain` required).
fn suggestion_to_json(s: &types::Suggestion) -> Option<serde_json::Value> {
    let domain = s.domain.as_deref()?;
    let mut obj = json!({ "domain": domain });
    let prices = s.prices.as_deref().unwrap_or(&[]);
    let term_1yr = term_for_period(prices, 1);
    let term_2yr = term_for_period(prices, 2);
    if let Some(t) = term_1yr {
        if let Some(p) = t.price.as_ref().and_then(format_money) {
            obj["price1Year"] = json!(p);
        }
        if let Some(r) = t.renewal_price.as_ref().and_then(format_money) {
            obj["renewalPrice1Year"] = json!(r);
        }
    }
    if let Some(t) = term_2yr {
        if let Some(p) = t.price.as_ref().and_then(format_money) {
            obj["price2Year"] = json!(p);
        }
        if let Some(r) = t.renewal_price.as_ref().and_then(format_money) {
            obj["renewalPrice2Year"] = json!(r);
        }
    }
    // Only emit `currency` when a code is present — never a JSON null (the
    // schema declares it an optional string, and null leaks into tables). Falls
    // back to `renewal_price`'s code when `price` is absent, so a renewal-only
    // term still gets a currency.
    let currency_code = term_1yr
        .or(term_2yr)
        .and_then(|t| t.price.as_ref().or(t.renewal_price.as_ref()))
        .and_then(|m| m.currency_code.as_ref());
    if let Some(code) = currency_code {
        obj["currency"] = json!(code.to_string());
    }
    Some(obj)
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_with_context(
        CommandSpec::new("suggest", "Suggest available domains for a query")
            .with_long(
                "Suggest available domains from a seed word, phrase, or domain. \
                 Narrow results with --tlds (repeatable), bound the name length \
                 with --length-min/--length-max, and cap the count with --limit \
                 (1-50).",
            )
            .with_system("domain")
            .with_tier(Tier::Read)
            .with_default_fields("domain,price1Year,renewalPrice1Year,currency")
            .with_output_schema::<DomainSuggestResult>()
            .with_scopes(&[DOMAINS_READ])
            .with_arg(
                clap::Arg::new("query")
                    .value_name("QUERY")
                    .required(true)
                    .help("Seed domain or keywords to base suggestions on"),
            )
            .with_arg(
                clap::Arg::new("tlds")
                    .long("tlds")
                    .value_name("TLD")
                    .action(clap::ArgAction::Append)
                    .help("Limit suggestions to these TLDs (repeatable)"),
            )
            .with_arg(
                // Local override of the engine's global `--limit`, typed
                // `i64` to match it: clap's `global(true)` propagates a
                // parsed value up into every ancestor `ArgMatches` under the
                // shared `"limit"` id, and cli-engine's global-flag parsing
                // always reads the root value back out as `i64` — a
                // differently-typed local override (e.g. `u64`) panics there
                // with a downcast mismatch. `allow_negative_numbers` (not the
                // broader `allow_hyphen_values`) lets a negative value reach
                // the range check below without also swallowing a following
                // flag's name as this arg's value when one is omitted. The
                // v3 suggestions endpoint hard-caps `pageSize` at 50
                // (DEVEX-883), so this command validates the bound itself
                // via clap instead of forwarding an out-of-range value the
                // server rejects with a raw `400 VALUE_OVER`, or silently
                // clamping it.
                clap::Arg::new("limit")
                    .long("limit")
                    .value_name("N")
                    .allow_negative_numbers(true)
                    .value_parser(clap::value_parser!(i64).range(1..=50))
                    .help("Maximum suggestions to return (1-50)"),
            )
            .with_arg(
                clap::Arg::new("length-min")
                    .long("length-min")
                    .value_name("N")
                    .value_parser(clap::value_parser!(i64).range(1..))
                    .help("Only suggest names at least this many characters"),
            )
            .with_arg(
                clap::Arg::new("length-max")
                    .long("length-max")
                    .value_name("N")
                    .value_parser(clap::value_parser!(i64).range(1..))
                    .help("Only suggest names at most this many characters"),
            ),
        |ctx| async move {
            let query = ctx
                .args
                .get("query")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned();
            let tlds = string_list(&ctx, "tlds");
            let limit = ctx
                .args
                .get("limit")
                .and_then(|v| v.as_i64())
                .and_then(nonzero);
            let length_min = ctx.args.get("length-min").and_then(|v| v.as_i64());
            let length_max = ctx.args.get("length-max").and_then(|v| v.as_i64());

            let debug = !ctx.middleware.debug.is_empty();
            let client = make_client(&ctx).await?;
            let mut req = client.suggest_domains().query(query.as_str());
            if let Some(page_size) = limit {
                req = req.page_size(page_size);
            }
            if !tlds.is_empty() {
                req = req.tlds(comma_joined(tlds));
            }
            if let Some(n) = length_min.and_then(nonzero) {
                req = req.length_min(n);
            }
            if let Some(n) = length_max.and_then(nonzero) {
                req = req.length_max(n);
            }
            let resp = match req.send().await {
                Ok(r) => r.into_inner(),
                Err(e) => return Err(api_error("domain suggestion", debug, e).await),
            };
            let suggestions: Vec<serde_json::Value> =
                resp.items.iter().filter_map(suggestion_to_json).collect();
            Ok(
                CommandResult::new(json!(suggestions)).with_next_actions(vec![
                    next_action("domain available <domain>", "Check a suggested domain")
                        .with_param("domain", NextActionParam::required()),
                ]),
            )
        },
    )
}

#[cfg(test)]
mod tests {
    use super::super::common::comma_joined;
    use super::{command, nonzero, suggestion_to_json};
    use domains_client::types;

    /// Builds a standalone `clap::Command` from the real `--limit` arg
    /// definition (not a re-declared copy), so this exercises the actual
    /// validation users hit, not a hand-maintained stand-in that could drift
    /// from it.
    fn suggest_clap_command() -> clap::Command {
        clap::Command::new("suggest").args(command().spec.args)
    }

    #[test]
    fn limit_rejects_out_of_range_and_negative_values() {
        for bad in ["0", "51", "-1"] {
            let err = suggest_clap_command()
                .try_get_matches_from(["suggest", "coffee", "--limit", bad])
                .expect_err("out-of-range --limit should be rejected");
            assert_eq!(
                err.kind(),
                clap::error::ErrorKind::ValueValidation,
                "--limit {bad} should fail range validation, got: {err}"
            );
        }
    }

    #[test]
    fn limit_accepts_the_full_valid_range() {
        for good in ["1", "50"] {
            suggest_clap_command()
                .try_get_matches_from(["suggest", "coffee", "--limit", good])
                .expect("value within the valid --limit range should be accepted");
        }
    }

    #[test]
    fn nonzero_maps_zero_and_negative_to_none() {
        // None of `--limit`/`--length-min`/`--length-max` have a clap
        // default, so they're simply absent from `ctx.args` when unset —
        // `nonzero` must still map 0/negative to "no param" rather than
        // panic, as a safety net (regression for the suggest crash).
        assert_eq!(nonzero(0), None);
        assert_eq!(nonzero(-5), None);
        assert_eq!(nonzero(5).map(|n| n.get()), Some(5));
    }

    #[tokio::test]
    async fn tlds_are_sent_as_a_single_comma_joined_query_param() {
        // Regression for DEVEX-882: the `tlds` query param is OpenAPI `style:
        // form, explode: false` — one comma-joined value. progenitor's generated
        // `tlds()` setter always seq-serializes a `Vec` as repeated `tlds=`
        // pairs, so callers must join multiple `--tlds` occurrences into a
        // single element before calling it, or the API replies `400
        // MISMATCH_FORMAT`.
        let server = httpmock::MockServer::start_async().await;
        let mock = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::GET)
                    .path("/v3/domains/suggestions")
                    .query_param("tlds", "com,net,io");
                then.status(200)
                    .json_body(serde_json::json!({ "items": [] }));
            })
            .await;
        let client =
            domains_client::client_with_auth(&server.base_url(), "Bearer tok", "test", "req-1")
                .expect("build client");

        let tlds = comma_joined(vec!["com".to_string(), "net".to_string(), "io".to_string()]);
        client
            .suggest_domains()
            .query("pizza")
            .tlds(tlds)
            .send()
            .await
            .expect("request succeeds");

        mock.assert_async().await;
    }

    fn money(value: i64, currency: &str) -> types::SimpleMoney {
        types::SimpleMoney {
            value: Some(value),
            currency_code: Some(types::CurrencyCode(currency.to_string())),
        }
    }

    fn term(period: u64, price: i64, renewal: i64) -> types::TermPrice {
        types::TermPrice {
            period: std::num::NonZeroU64::new(period),
            price: Some(money(price, "USD")),
            renewal_price: Some(money(renewal, "USD")),
            term: Some(types::Term::Year),
        }
    }

    #[test]
    fn flattens_1_and_2_year_terms_into_scalar_fields() {
        // Mirrors the API's example response for a suggestion with both terms.
        let suggestion = types::Suggestion {
            domain: Some("kirklandfreshtreats.shop".to_string()),
            inventory: Some(types::InventoryType::Registry),
            prices: Some(vec![term(1, 99, 3799), term(2, 6098, 7598)]),
        };
        let obj = suggestion_to_json(&suggestion).expect("domain is present");
        assert_eq!(obj["domain"], "kirklandfreshtreats.shop");
        assert_eq!(obj["price1Year"], "0.99");
        assert_eq!(obj["renewalPrice1Year"], "37.99");
        assert_eq!(obj["price2Year"], "60.98");
        assert_eq!(obj["renewalPrice2Year"], "75.98");
        assert_eq!(obj["currency"], "USD");
    }

    #[test]
    fn currency_falls_back_to_renewal_price_when_price_is_absent() {
        // A term with only a renewal price (no indicative price) must still
        // surface a currency code, sourced from the renewal price itself.
        let renewal_only = types::TermPrice {
            period: std::num::NonZeroU64::new(1),
            price: None,
            renewal_price: Some(money(3799, "USD")),
            term: Some(types::Term::Year),
        };
        let suggestion = types::Suggestion {
            domain: Some("example.com".to_string()),
            inventory: Some(types::InventoryType::Registry),
            prices: Some(vec![renewal_only]),
        };
        let obj = suggestion_to_json(&suggestion).expect("domain is present");
        assert_eq!(obj["renewalPrice1Year"], "37.99");
        assert!(obj.get("price1Year").is_none());
        assert_eq!(obj["currency"], "USD");
    }

    #[test]
    fn no_domain_is_skipped() {
        let suggestion = types::Suggestion {
            domain: None,
            inventory: None,
            prices: None,
        };
        assert!(suggestion_to_json(&suggestion).is_none());
    }
}
