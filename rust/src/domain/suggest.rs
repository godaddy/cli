//! `gddy domain suggest` — suggest available domains for a query (v3).

use cli_engine::{
    CommandResult, CommandSpec, NextAction, NextActionParam, RuntimeCommandSpec, Tier,
};
use serde_json::json;

use super::common::{api_error, format_money, headline_price, make_client, string_list};
use crate::output_schema::output_schema;
use crate::scopes::DOMAINS_READ;

// The handler emits a transformed per-item shape (`listPrice` is a formatted
// string from SimpleMoney, not the raw `types::Suggestion`), so declare the
// schema to match what's actually returned for `--schema`/help.
output_schema!(DomainSuggestResult {
    "domain": "string";
    // Present only when the API returns a formattable list price.
    "listPrice": "string", optional;
});

/// Convert a count-style flag value to the `NonZeroU64` the suggest query params
/// expect, or `None` when it's absent/zero/negative (so the param is simply
/// omitted). Total — never panics — because the value can arrive as `0` (e.g. the
/// engine's global `--limit` default is `0`), which must mean "unset", not abort.
fn nonzero(n: i64) -> Option<std::num::NonZeroU64> {
    u64::try_from(n).ok().and_then(std::num::NonZeroU64::new)
}

pub(super) fn command() -> RuntimeCommandSpec {
    RuntimeCommandSpec::new_with_context(
        CommandSpec::new("suggest", "Suggest available domains for a query")
            .with_long(
                "Suggest available domains from a seed word, phrase, or domain. \
                 Narrow results with --tlds (repeatable) and bound the name length \
                 with --length-min/--length-max. The global --limit caps how many \
                 suggestions are requested and shown.",
            )
            .with_system("domain")
            .with_tier(Tier::Read)
            .with_default_fields("domain")
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
            // Note: no per-command `--limit` — cli-engine registers a global
            // `--limit` (client-side result cap). We reuse its value as the v3
            // `pageSize` below, so a single `--limit` drives both.
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
            // The engine's global `--limit` (0 when unset) doubles as the v3
            // server-side pageSize; `nonzero` maps 0/absent to "no pageSize".
            let limit = nonzero(ctx.middleware.limit);
            let length_min = ctx.args.get("length-min").and_then(|v| v.as_i64());
            let length_max = ctx.args.get("length-max").and_then(|v| v.as_i64());

            let debug = !ctx.middleware.debug.is_empty();
            let client = make_client(&ctx).await?;
            let mut req = client.suggest_domains().query(query.as_str());
            if let Some(page_size) = limit {
                req = req.page_size(page_size);
            }
            if !tlds.is_empty() {
                req = req.tlds(tlds);
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
            let suggestions: Vec<serde_json::Value> = resp
                .items
                .into_iter()
                .filter_map(|s| {
                    // Skip any suggestion without a domain so every emitted object
                    // matches the schema (`domain` required) and projects cleanly.
                    let domain = s.domain?;
                    let mut obj = json!({ "domain": domain });
                    // v3 returns indicative pricing per term; surface the headline
                    // (1-year, else first) term's price as the scalar `listPrice`.
                    if let Some(p) = s
                        .prices
                        .as_deref()
                        .and_then(headline_price)
                        .and_then(|t| t.price.as_ref())
                        .and_then(format_money)
                    {
                        obj["listPrice"] = json!(p);
                    }
                    Some(obj)
                })
                .collect();
            Ok(
                CommandResult::new(json!(suggestions)).with_next_actions(vec![
                    NextAction::new("domain available <domain>", "Check a suggested domain")
                        .with_param("domain", NextActionParam::required()),
                ]),
            )
        },
    )
}

#[cfg(test)]
mod tests {
    use super::nonzero;

    #[test]
    fn nonzero_maps_zero_and_negative_to_none() {
        // The engine's global `--limit` arrives as 0 when unset — must be "no
        // pageSize", never a panic (regression for the suggest crash).
        assert_eq!(nonzero(0), None);
        assert_eq!(nonzero(-5), None);
        assert_eq!(nonzero(5).map(|n| n.get()), Some(5));
    }
}
