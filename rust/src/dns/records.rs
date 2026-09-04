//! Record building, fetching, and argument helpers shared by every `dns`
//! subcommand: type/name validation, the `RecordOptions` bundle, converting
//! CLI input into v3 `DnsRecord`s, and the paginated `list_dns_records` fetch.

use cli_engine::{CliCoreError, NextAction, NextActionParam};

use crate::domain::api_error;
use crate::next_action::next_action;

use domains_client::types;

/// DNS record types the CLI can create/replace/delete via v3 (`add`/`set`/`delete`).
/// NS and SOA are registry-managed / read-only, so they're excluded. v3's
/// `DNSRecordType` is otherwise an open string; this list is the CLI's guardrail
/// against typos and includes `CAA` and GoDaddy's `ALIAS` extension.
pub(super) const WRITABLE_TYPES: &[&str] = &[
    "A", "AAAA", "ALIAS", "CAA", "CNAME", "HTTPS", "MX", "SRV", "SVCB", "TLSA", "TXT",
];
/// Record types accepted by the `list` filter — the writable set plus the
/// read-only NS/SOA, which are listable even though they can't be modified.
pub(super) const LISTABLE_TYPES: &[&str] = &[
    "A", "AAAA", "ALIAS", "CAA", "CNAME", "HTTPS", "MX", "NS", "SOA", "SRV", "SVCB", "TLSA", "TXT",
];
/// Default TTL (seconds) for `dns add`/`set` when `--ttl` is omitted (v3 requires a ttl).
pub(super) const DEFAULT_TTL: i64 = 3600;
/// Page size for the paginated v3 list; the handler pages through until every
/// matching record is collected.
const LIST_PAGE_SIZE: i64 = 100;

/// clap value-parser for a mutating `--type` (`add`/`set`/`delete`): validate
/// against [`WRITABLE_TYPES`] and return the canonical upper-case wire string.
/// The read-only NS/SOA get a clear "managed by GoDaddy" reason. Validating in
/// clap rejects invalid input at parse time — before `--dry-run`/auth short-circuits.
pub(super) fn parse_write_type_arg(raw: &str) -> Result<String, String> {
    let upper = raw.to_ascii_uppercase();
    if WRITABLE_TYPES.contains(&upper.as_str()) {
        return Ok(upper);
    }
    if matches!(upper.as_str(), "NS" | "SOA") {
        Err(format!(
            "{upper} records are managed by GoDaddy and can't be created, replaced, or deleted; \
             writable types: {}",
            WRITABLE_TYPES.join(", ")
        ))
    } else {
        Err(format!(
            "invalid record type {raw:?}; expected one of {}",
            WRITABLE_TYPES.join(", ")
        ))
    }
}

/// clap value-parser for the `list` `--type` filter: [`LISTABLE_TYPES`] (the
/// writable set plus read-only NS/SOA), upper-cased.
pub(super) fn parse_list_type_arg(raw: &str) -> Result<String, String> {
    let upper = raw.to_ascii_uppercase();
    if LISTABLE_TYPES.contains(&upper.as_str()) {
        Ok(upper)
    } else {
        Err(format!(
            "invalid record type {raw:?}; expected one of {}",
            LISTABLE_TYPES.join(", ")
        ))
    }
}

/// Optional record fields shared by `add` and `set`, including the CAA-only
/// `flag`/`tag`, the TLSA-only `usage`/`selector`/`matching_type`, and the
/// HTTPS/SVCB-only `parameters` (SvcParams).
pub(super) struct RecordOptions {
    pub(super) ttl: Option<i64>,
    pub(super) priority: Option<i64>,
    pub(super) port: Option<i64>,
    pub(super) weight: Option<i64>,
    pub(super) protocol: Option<String>,
    pub(super) service: Option<String>,
    pub(super) flag: Option<i64>,
    pub(super) tag: Option<String>,
    pub(super) usage: Option<i64>,
    pub(super) selector: Option<i64>,
    pub(super) matching_type: Option<i64>,
    pub(super) parameters: Option<String>,
}

impl RecordOptions {
    pub(super) fn from_write_args(args: &RecordWriteArgs) -> Self {
        RecordOptions {
            ttl: args.ttl,
            priority: args.priority,
            port: args.port,
            weight: args.weight,
            protocol: args.protocol.clone(),
            service: args.service.clone(),
            flag: args.flag,
            tag: args.tag.clone(),
            usage: args.usage,
            selector: args.selector,
            matching_type: args.matching_type,
            parameters: args.parameters.clone(),
        }
    }
}

/// Shared flags for the mutating commands (`add`/`set`): required type/name and
/// the repeatable `--data`, plus the optional record fields.
#[derive(Debug, Clone, clap::Args)]
pub(super) struct RecordWriteArgs {
    /// Domain whose records to modify (e.g. example.com).
    #[arg(value_name = "DOMAIN")]
    pub(super) domain: String,

    /// Record type (A, AAAA, ALIAS, CAA, CNAME, HTTPS, MX, SRV, SVCB, TLSA, TXT).
    #[arg(long = "type", value_name = "TYPE", value_parser = parse_write_type_arg)]
    pub(super) record_type: String,

    /// Record name relative to the domain (e.g. www, @ for the apex).
    #[arg(long, value_name = "NAME")]
    pub(super) name: String,

    /// Record value (repeatable for multiple records on the same name). For
    /// TLSA, this is the hex-encoded certificate association data; use
    /// `--usage`/`--selector`/`--matching-type` for the rest.
    #[arg(long, value_name = "VALUE", required = true)]
    pub(super) data: Vec<String>,

    /// Time-to-live in seconds (defaults to 3600 when omitted).
    #[arg(long, value_name = "SECONDS", value_parser = clap::value_parser!(i64).range(1..))]
    pub(super) ttl: Option<i64>,

    /// Record priority (MX, SRV, HTTPS, and SVCB only). For HTTPS/SVCB, 0
    /// means AliasMode.
    #[arg(long, value_name = "N", value_parser = clap::value_parser!(i64).range(0..=65535))]
    pub(super) priority: Option<i64>,

    /// Service port (SRV and TLSA only).
    #[arg(long, value_name = "PORT", value_parser = clap::value_parser!(i64).range(1..=65535))]
    pub(super) port: Option<i64>,

    /// Record weight (SRV only).
    #[arg(long, value_name = "N", value_parser = clap::value_parser!(i64).range(0..=65535))]
    pub(super) weight: Option<i64>,

    /// Service protocol, e.g. _tcp (SRV and TLSA only).
    #[arg(long, value_name = "PROTO")]
    pub(super) protocol: Option<String>,

    /// Service type (SRV only).
    #[arg(long, value_name = "SERVICE")]
    pub(super) service: Option<String>,

    /// CAA flag byte, 0-255 (CAA only; 0 non-critical, 128 critical).
    #[arg(long, value_name = "N", value_parser = clap::value_parser!(i64).range(0..=255))]
    pub(super) flag: Option<i64>,

    // A CAA record needs a tag; enforce it at parse time (before auth). The
    // reverse guard — flag/tag only valid for CAA — lives in the handler
    // (`validate_caa_fields`), which clap can't express.
    /// CAA property tag, e.g. issue/issuewild/iodef (CAA only; required for CAA).
    #[arg(long, value_name = "TAG", required_if_eq("record_type", "CAA"))]
    pub(super) tag: Option<String>,

    /// TLSA certificate usage, 0-3 (RFC 6698 §2.1.1; TLSA only; required for
    /// TLSA). 0 PKIX-TA, 1 PKIX-EE, 2 DANE-TA, 3 DANE-EE.
    #[arg(long = "usage", value_name = "N", value_parser = clap::value_parser!(i64).range(0..=3), required_if_eq("record_type", "TLSA"))]
    pub(super) usage: Option<i64>,

    /// TLSA selector, 0-1 (RFC 6698 §2.1.2; TLSA only; required for TLSA). 0
    /// full certificate, 1 SubjectPublicKeyInfo.
    #[arg(long = "selector", value_name = "N", value_parser = clap::value_parser!(i64).range(0..=1), required_if_eq("record_type", "TLSA"))]
    pub(super) selector: Option<i64>,

    /// TLSA matching type, 0-2 (RFC 6698 §2.1.3; TLSA only; required for
    /// TLSA). 0 exact match, 1 SHA-256, 2 SHA-512.
    #[arg(long = "matching-type", value_name = "N", value_parser = clap::value_parser!(i64).range(0..=2), required_if_eq("record_type", "TLSA"))]
    pub(super) matching_type: Option<i64>,

    /// SvcParams for HTTPS/SVCB records (RFC 9460), e.g. "alpn=h2,h3
    /// port=8443" (HTTPS/SVCB only).
    #[arg(long, value_name = "PARAMS")]
    pub(super) parameters: Option<String>,
}

/// Validate the CAA-specific fields against the record type. A CAA record needs a
/// `--tag` (`--data` carries the CA domain / value); `--flag`/`--tag` are
/// meaningless for other types. Pure so it's unit-testable and runs before any
/// network call.
pub(super) fn validate_caa_fields(record_type: &str, opts: &RecordOptions) -> Result<(), String> {
    if record_type == "CAA" {
        if opts.tag.as_deref().map(str::trim).unwrap_or("").is_empty() {
            return Err(
                "CAA records require --tag (e.g. issue, issuewild, iodef); --data carries the \
                 CA domain / value"
                    .to_string(),
            );
        }
    } else if opts.flag.is_some() || opts.tag.is_some() {
        return Err(format!(
            "--flag/--tag are only valid for CAA records, not {record_type}"
        ));
    }
    Ok(())
}

/// Validate the TLSA-specific fields against the record type. `--usage`/
/// `--selector`/`--matching-type` being present when `record_type` is TLSA is
/// already enforced by clap (`required_if_eq`); this only guards the reverse —
/// they're meaningless for other types. Pure so it's unit-testable and runs
/// before any network call.
pub(super) fn validate_tlsa_fields(record_type: &str, opts: &RecordOptions) -> Result<(), String> {
    if record_type != "TLSA"
        && (opts.usage.is_some() || opts.selector.is_some() || opts.matching_type.is_some())
    {
        return Err(format!(
            "--usage/--selector/--matching-type are only valid for TLSA records, not {record_type}"
        ));
    }
    Ok(())
}

/// Validate the HTTPS/SVCB-specific `--parameters` (SvcParams) field against
/// the record type — meaningless for anything else. Pure so it's
/// unit-testable and runs before any network call.
pub(super) fn validate_svcb_fields(record_type: &str, opts: &RecordOptions) -> Result<(), String> {
    if opts.parameters.is_some() && !matches!(record_type, "HTTPS" | "SVCB") {
        return Err(format!(
            "--parameters is only valid for HTTPS/SVCB records, not {record_type}"
        ));
    }
    Ok(())
}

/// Build one v3 `DnsRecord` from a `--data` value + shared options. `ttl` defaults
/// to [`DEFAULT_TTL`] (v3 requires it). SRV/MX numerics convert into v3's `u16`
/// and the CAA `flag`/TLSA `usage`/`selector`/`matchingType` into `u8` (clap
/// already bounds all four ranges). TLSA doesn't use `data` — v3 wants its
/// value under `certificateData` instead — so `data`'s value moves there.
pub(super) fn v3_record(
    name: &str,
    ty: &str,
    data: &str,
    opts: &RecordOptions,
) -> types::DnsRecord {
    let to_u16 = |v: Option<i64>| v.and_then(|n| u16::try_from(n).ok());
    let to_u8 = |v: Option<i64>| v.and_then(|n| u8::try_from(n).ok());
    let is_tlsa = ty == "TLSA";
    types::DnsRecord {
        certificate_data: is_tlsa.then(|| data.to_owned()),
        matching_type: is_tlsa
            .then(|| to_u8(opts.matching_type))
            .flatten()
            .map(types::TlsaMatchingType),
        selector: is_tlsa
            .then(|| to_u8(opts.selector))
            .flatten()
            .map(types::TlsaSelector),
        usage: is_tlsa
            .then(|| to_u8(opts.usage))
            .flatten()
            .map(types::TlsaUsage),
        data: (!is_tlsa).then(|| data.to_owned()),
        flag: opts.flag.and_then(|n| u8::try_from(n).ok()),
        name: name.to_owned(),
        parameters: opts.parameters.clone(),
        port: to_u16(opts.port),
        priority: to_u16(opts.priority),
        protocol: opts.protocol.clone(),
        record_id: None,
        service: opts.service.clone(),
        tag: opts.tag.clone(),
        ttl: opts.ttl.unwrap_or(DEFAULT_TTL),
        type_: types::DnsRecordType(ty.to_owned()),
        weight: to_u16(opts.weight),
    }
}

/// Build the v3 `DnsRecord`s for `add` — one per `--data` value (parallel to it).
pub(super) fn v3_records(
    name: &str,
    ty: &str,
    data: &[String],
    opts: &RecordOptions,
) -> Vec<types::DnsRecord> {
    data.iter().map(|d| v3_record(name, ty, d, opts)).collect()
}

/// The user-facing "value" of a fetched v3 `DnsRecord`: `data` for every type
/// except TLSA, which carries its value in `certificateData` instead (see
/// [`v3_record`]'s TLSA branch). Conflict diagnosis, delete/set reporting, and
/// exact-duplicate detection all need "the value" regardless of which wire
/// field holds it, so they read through this rather than `data` directly.
pub(super) fn record_value(rec: &types::DnsRecord) -> Option<&str> {
    rec.data.as_deref().or(rec.certificate_data.as_deref())
}

/// List every v3 DNS record for a zone matching the optional `type`/`name`
/// filters, paging through the collection (v3 list is paginated). Shared by
/// `list`, `set`, and `delete` — the latter two need the matching records' ids.
pub(super) async fn fetch_records(
    client: &domains_client::Client,
    zone: &str,
    type_: Option<&str>,
    name: Option<&str>,
    debug: bool,
) -> Result<Vec<types::DnsRecord>, CliCoreError> {
    let page_size = std::num::NonZeroU64::new(LIST_PAGE_SIZE as u64)
        .expect("LIST_PAGE_SIZE is a positive constant");
    let mut all = Vec::new();
    let mut page: u64 = 1;
    loop {
        let page_nz = std::num::NonZeroU64::new(page).expect("page starts at 1 and only grows");
        let mut req = client
            .list_dns_records()
            .zone(zone)
            .page(page_nz)
            .page_size(page_size)
            .total_required(true);
        if let Some(t) = type_ {
            req = req.type_(types::DnsRecordType(t.to_owned()));
        }
        if let Some(n) = name {
            req = req.name(n);
        }
        let body = match req.send().await {
            Ok(r) => r.into_inner(),
            Err(e) => return Err(api_error("listing DNS records", debug, e).await),
        };
        let items = body.items.unwrap_or_default();
        let got = items.len();
        all.extend(items);
        // Last page when the API's total_pages is reached, or (absent that) a
        // short/empty page came back.
        let last = body
            .total_pages
            .map(|tp| page >= tp.get())
            .unwrap_or(got < LIST_PAGE_SIZE as usize);
        if last || got == 0 {
            break;
        }
        page += 1;
    }
    Ok(all)
}

/// Next action pointing back at `dns list` to verify a write, pre-filled with
/// the domain/type/name the write just touched.
pub(super) fn verify_with_list_action(domain: &str, record_type: &str, name: &str) -> NextAction {
    next_action(
        "dns list <domain> --type <type> --name <name>",
        "Verify the records for this type+name",
    )
    .with_param("domain", NextActionParam::value(domain))
    .with_param("type", NextActionParam::value(record_type))
    .with_param("name", NextActionParam::value(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> RecordOptions {
        RecordOptions {
            ttl: None,
            priority: None,
            port: None,
            weight: None,
            protocol: None,
            service: None,
            flag: None,
            tag: None,
            usage: None,
            selector: None,
            matching_type: None,
            parameters: None,
        }
    }

    #[test]
    fn parse_write_type_arg_accepts_writable_incl_caa_alias_rejects_ns_soa() {
        assert_eq!(parse_write_type_arg("aaaa").expect("valid"), "AAAA");
        assert_eq!(parse_write_type_arg("caa").expect("valid"), "CAA");
        assert_eq!(parse_write_type_arg("Alias").expect("valid"), "ALIAS");
        assert_eq!(parse_write_type_arg("https").expect("valid"), "HTTPS");
        assert_eq!(parse_write_type_arg("svcb").expect("valid"), "SVCB");
        assert_eq!(parse_write_type_arg("tlsa").expect("valid"), "TLSA");
        // NS/SOA are registry-managed / read-only → rejected with a clear reason.
        for ty in ["NS", "soa"] {
            let err = parse_write_type_arg(ty).expect_err("read-only");
            assert!(err.contains("managed by GoDaddy"), "got: {err}");
        }
        let err = parse_write_type_arg("bogus").expect_err("should reject");
        assert!(err.contains("invalid record type"), "got: {err}");
    }

    #[test]
    fn parse_list_type_arg_also_accepts_ns_soa() {
        assert_eq!(parse_list_type_arg("caa").expect("valid"), "CAA");
        assert_eq!(parse_list_type_arg("ns").expect("valid"), "NS");
        assert_eq!(parse_list_type_arg("SOA").expect("valid"), "SOA");
        assert!(parse_list_type_arg("bogus").is_err());
    }

    #[test]
    fn v3_records_builds_one_per_data_value_with_default_ttl() {
        let recs = v3_records(
            "www",
            "A",
            &["1.2.3.4".to_string(), "5.6.7.8".to_string()],
            &opts(),
        );
        assert_eq!(recs.len(), 2);
        assert_eq!(recs[0].name, "www");
        assert_eq!(recs[0].type_.as_str(), "A");
        assert_eq!(recs[0].data.as_deref(), Some("1.2.3.4"));
        // v3 requires a ttl; an omitted --ttl falls back to the default.
        assert_eq!(recs[0].ttl, DEFAULT_TTL);
        assert_eq!(recs[1].data.as_deref(), Some("5.6.7.8"));
    }

    #[test]
    fn v3_record_carries_srv_and_caa_fields() {
        let mut srv_opts = opts();
        srv_opts.ttl = Some(600);
        srv_opts.priority = Some(10);
        srv_opts.port = Some(443);
        srv_opts.weight = Some(5);
        let srv = v3_record("_sip", "SRV", "sip.example.com", &srv_opts);
        assert_eq!(srv.ttl, 600);
        assert_eq!(srv.priority, Some(10));
        assert_eq!(srv.port, Some(443));
        assert_eq!(srv.weight, Some(5));

        let mut caa_opts = opts();
        caa_opts.flag = Some(128);
        caa_opts.tag = Some("issue".to_string());
        let caa = v3_record("@", "CAA", "letsencrypt.org", &caa_opts);
        assert_eq!(caa.flag, Some(128));
        assert_eq!(caa.tag.as_deref(), Some("issue"));
    }

    #[test]
    fn caa_fields_are_required_for_caa_and_rejected_otherwise() {
        // CAA without --tag → rejected.
        let mut caa_no_tag = opts();
        caa_no_tag.flag = Some(0);
        let err = validate_caa_fields("CAA", &caa_no_tag).expect_err("CAA needs a tag");
        assert!(err.contains("--tag"), "got: {err}");
        // CAA with --tag → ok.
        let mut caa = opts();
        caa.tag = Some("issue".to_string());
        assert!(validate_caa_fields("CAA", &caa).is_ok());
        // --flag/--tag on a non-CAA type → rejected.
        let mut a = opts();
        a.tag = Some("issue".to_string());
        let err = validate_caa_fields("A", &a).expect_err("flag/tag are CAA-only");
        assert!(err.contains("only valid for CAA"), "got: {err}");
        // A non-CAA type with no CAA fields → ok.
        assert!(validate_caa_fields("A", &opts()).is_ok());
    }

    #[test]
    fn v3_record_carries_https_svcb_and_tlsa_fields() {
        let mut https_opts = opts();
        https_opts.priority = Some(1);
        https_opts.parameters = Some("alpn=h2,h3".to_string());
        let https = v3_record("@", "HTTPS", ".", &https_opts);
        assert_eq!(https.priority, Some(1));
        assert_eq!(https.parameters.as_deref(), Some("alpn=h2,h3"));
        assert_eq!(https.data.as_deref(), Some("."));
        assert_eq!(https.certificate_data, None);

        let mut tlsa_opts = opts();
        tlsa_opts.usage = Some(3);
        tlsa_opts.selector = Some(1);
        tlsa_opts.matching_type = Some(1);
        tlsa_opts.protocol = Some("_tcp".to_string());
        tlsa_opts.port = Some(443);
        let cert = "d2abde240d7cd3ee6b4b28c54df034b97983a1d16e8a410e4561cb106618e971";
        let tlsa = v3_record("www", "TLSA", cert, &tlsa_opts);
        // TLSA doesn't use `data` — the value moves to `certificateData`.
        assert_eq!(tlsa.data, None);
        assert_eq!(tlsa.certificate_data.as_deref(), Some(cert));
        assert_eq!(tlsa.usage.map(|u| u.0), Some(3));
        assert_eq!(tlsa.selector.map(|s| s.0), Some(1));
        assert_eq!(tlsa.matching_type.map(|m| m.0), Some(1));
        assert_eq!(tlsa.protocol.as_deref(), Some("_tcp"));
        assert_eq!(tlsa.port, Some(443));
    }

    #[test]
    fn tlsa_fields_are_required_for_tlsa_and_rejected_otherwise() {
        // --usage/--selector/--matching-type on a non-TLSA type → rejected.
        let mut a = opts();
        a.usage = Some(3);
        let err = validate_tlsa_fields("A", &a).expect_err("TLSA fields are TLSA-only");
        assert!(err.contains("only valid for TLSA"), "got: {err}");
        // TLSA with the fields set → ok.
        let mut tlsa = opts();
        tlsa.usage = Some(3);
        tlsa.selector = Some(1);
        tlsa.matching_type = Some(1);
        assert!(validate_tlsa_fields("TLSA", &tlsa).is_ok());
        // A non-TLSA type with no TLSA fields → ok.
        assert!(validate_tlsa_fields("A", &opts()).is_ok());
    }

    #[test]
    fn svcb_parameters_are_rejected_for_other_types() {
        let mut a = opts();
        a.parameters = Some("alpn=h2".to_string());
        let err = validate_svcb_fields("A", &a).expect_err("parameters are HTTPS/SVCB-only");
        assert!(err.contains("only valid for HTTPS/SVCB"), "got: {err}");
        assert!(validate_svcb_fields("HTTPS", &a).is_ok());
        assert!(validate_svcb_fields("SVCB", &a).is_ok());
        assert!(validate_svcb_fields("A", &opts()).is_ok());
    }
}
