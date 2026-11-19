use std::collections::BTreeSet;
use std::num::NonZeroU64;

use cli_engine::Result;
use shopping_client::types::{Item, LineItem, UcpRefsSchemaLineItem};

use crate::domain::common::validate_domain_name;
use crate::error::GddyError;

pub(crate) const DOMAIN_CATEGORY: &str = "domain";

pub(crate) fn line_items(
    items: &[String],
    domains: &[String],
) -> Result<Vec<UcpRefsSchemaLineItem>> {
    let mut line_items = items
        .iter()
        .map(|item| {
            let (id, quantity) = parse_item(item)?;
            Ok(line_item(id.to_owned(), None, quantity))
        })
        .collect::<Result<Vec<_>>>()?;
    let mut seen = BTreeSet::new();
    for domain in domains {
        let id = parse_domain(domain)?;
        let name = id.split_once(':').map_or(id.as_str(), |(name, _)| name);
        if !seen.insert(name.to_owned()) {
            return Err(GddyError::validation(format!(
                "--domain {name:?} is listed more than once"
            ))
            .with_fix("List each domain once. Append :<years>yr to choose a registration term.")
            .into_cli_error());
        }
        line_items.push(line_item(
            id,
            Some(DOMAIN_CATEGORY.to_owned()),
            NonZeroU64::MIN,
        ));
    }
    Ok(line_items)
}

fn line_item(id: String, category: Option<String>, quantity: NonZeroU64) -> UcpRefsSchemaLineItem {
    UcpRefsSchemaLineItem(LineItem {
        item: Some(Item {
            id: Some(id),
            category,
            ..Default::default()
        }),
        quantity: Some(quantity),
        ..Default::default()
    })
}

/// A checkout response names a domain line by `common_name`; its `id` is a
/// generic offer name the API rejects on resend. Rebuilds the request id,
/// keeping a multi-year term because an update replaces the whole cart and a
/// bare name would reset it to one year.
pub(crate) fn domain_line_id(item: &Item) -> Option<String> {
    let name = item
        .common_name
        .as_deref()
        .filter(|name| !name.trim().is_empty())?;
    let years = item
        .term
        .as_ref()
        .filter(|term| term.term_type.as_deref() == Some("YEAR"))
        .and_then(|term| term.number_of_terms)
        .filter(|years| years.get() > 1);
    Some(match years {
        Some(years) => format!("{name}:{years}yr"),
        None => name.to_owned(),
    })
}

/// `FQDN[:Nyr]` → the ASCII (punycode) FQDN, with the term suffix kept as typed.
pub(crate) fn parse_domain(value: &str) -> Result<String> {
    let value = value.trim();
    let (name, years) = match value.rsplit_once(':') {
        Some((name, term)) => (name, Some(parse_years(value, term)?)),
        None => (value, None),
    };
    let name = validate_domain_name(name).map_err(|error| {
        GddyError::validation(format!("invalid --domain {value:?}: {error}"))
            .with_fix("Use a full domain name such as example.com, optionally followed by :<years>yr (for example, example.com:2yr).")
            .into_cli_error()
    })?;
    Ok(match years {
        Some(years) => format!("{name}:{years}yr"),
        None => name,
    })
}

fn parse_years(value: &str, term: &str) -> Result<NonZeroU64> {
    term.strip_suffix("yr")
        .and_then(|years| years.parse::<NonZeroU64>().ok())
        .ok_or_else(|| {
            GddyError::validation(format!(
                "invalid --domain {value:?}: the term after ':' must be a number of years such as 2yr"
            ))
            .with_fix("Use a full domain name such as example.com, optionally followed by :<years>yr (for example, example.com:2yr).")
            .into_cli_error()
        })
}

fn parse_item(value: &str) -> Result<(&str, NonZeroU64)> {
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
    let quantity = NonZeroU64::new(quantity).ok_or_else(|| {
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

#[cfg(test)]
mod tests {
    use shopping_client::types::{Item, ItemTerm};

    use super::{domain_line_id, line_items, parse_domain};

    fn ids_and_categories(
        items: &[String],
        domains: &[String],
    ) -> Vec<(String, Option<String>, u64)> {
        line_items(items, domains)
            .expect("valid line items")
            .into_iter()
            .map(|line| {
                let item = line.0.item.expect("item");
                (
                    item.id.expect("id"),
                    item.category,
                    line.0.quantity.expect("quantity").get(),
                )
            })
            .collect()
    }

    #[test]
    fn builds_domain_line_items_after_purchase_options() {
        assert_eq!(
            ids_and_categories(
                &["wsb-tier1=2".to_owned()],
                &["Example.com".to_owned(), "shop.example.net:2yr".to_owned()]
            ),
            vec![
                ("wsb-tier1".to_owned(), None, 2),
                ("example.com".to_owned(), Some("domain".to_owned()), 1),
                (
                    "shop.example.net:2yr".to_owned(),
                    Some("domain".to_owned()),
                    1
                ),
            ]
        );
    }

    #[test]
    fn domain_line_item_serializes_with_category_and_fqdn() {
        let body =
            serde_json::to_value(line_items(&[], &["example.com".to_owned()]).expect("domain"))
                .expect("serialize");
        assert_eq!(
            body,
            serde_json::json!([{
                "item": {"id": "example.com", "category": "domain"},
                "quantity": 1
            }])
        );
    }

    #[test]
    fn domain_is_sent_in_punycode() {
        assert_eq!(
            parse_domain(" bücher.de ").expect("idn"),
            "xn--bcher-kva.de"
        );
        assert_eq!(
            parse_domain("bücher.de:3yr").expect("idn with term"),
            "xn--bcher-kva.de:3yr"
        );
    }

    #[test]
    fn rejects_malformed_domains_and_terms() {
        for bad in [
            "",
            "example",
            "https://example.com",
            "example.com/path",
            "192.0.2.1",
            "example.com:0yr",
            "example.com:2mo",
            "example.com:yr",
            "example.com:",
        ] {
            assert!(parse_domain(bad).is_err(), "{bad:?} should be rejected");
        }
    }

    fn response_item(common_name: Option<&str>, years: Option<u64>, unit: &str) -> Item {
        Item {
            id: Some("domain".to_owned()),
            common_name: common_name.map(str::to_owned),
            term: years.map(|years| ItemTerm {
                number_of_terms: std::num::NonZeroU64::new(years),
                term_type: Some(unit.to_owned()),
            }),
            ..Default::default()
        }
    }

    #[test]
    fn rebuilds_domain_line_id_from_common_name_and_term() {
        assert_eq!(
            domain_line_id(&response_item(Some("example.com"), Some(1), "YEAR")).as_deref(),
            Some("example.com")
        );
        assert_eq!(
            domain_line_id(&response_item(Some("example.com"), Some(3), "YEAR")).as_deref(),
            Some("example.com:3yr")
        );
        assert_eq!(
            domain_line_id(&response_item(Some("example.com"), None, "YEAR")).as_deref(),
            Some("example.com")
        );
        assert_eq!(domain_line_id(&response_item(None, Some(2), "YEAR")), None);
        assert_eq!(
            domain_line_id(&response_item(Some(" "), None, "YEAR")),
            None
        );
    }

    #[test]
    fn rejects_the_same_domain_twice() {
        assert!(
            line_items(
                &[],
                &["example.com".to_owned(), "EXAMPLE.com:2yr".to_owned()]
            )
            .is_err()
        );
    }

    #[test]
    fn rejects_invalid_purchase_option_items() {
        for bad in ["product=0", "product=two", "=2", " "] {
            assert!(line_items(&[bad.to_owned()], &[]).is_err(), "{bad:?}");
        }
    }
}
