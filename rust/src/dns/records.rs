//! Record building, fetching, and argument helpers shared by every `dns`
//! subcommand: type/name validation, the `RecordOptions` bundle, converting
//! CLI input into v3 `DnsRecord`s, and the paginated `list_dns_records` fetch.

use cli_engine::{CliCoreError, CommandResult, NextAction, NextActionParam};
use serde_json::Value;

use crate::domain::api_error;
use crate::idn::DomainForm;
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
/// The output fields of a DNS write that hold the zone and the record name.
pub(super) const ZONE_AND_NAME: &[&str] = &["domain", "name"];

/// Page size for the paginated v3 list; the handler pages through until every
/// matching record is collected.
const LIST_PAGE_SIZE: std::num::NonZeroU64 =
    std::num::NonZeroU64::new(100).expect("LIST_PAGE_SIZE is nonzero");

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

/// Flags below that clap only requires conditionally, via `required_if_eq`.
/// clap doesn't expose that relation publicly, so `spec_cmd` reads this
/// table by hand. Keep in sync with the attributes below.
#[cfg_attr(not(feature = "dev-portal-spec"), allow(dead_code))]
pub(crate) const CONDITIONALLY_REQUIRED_FLAGS: &[(&str, &str)] = &[
    ("tag", "required when --type is CAA"),
    ("usage", "required when --type is TLSA"),
    ("selector", "required when --type is TLSA"),
    ("matching-type", "required when --type is TLSA"),
];

/// Shared flags for the mutating commands (`add`/`set`): required type/name and
/// the repeatable `--data`, plus the optional record fields.
#[derive(Debug, Clone, clap::Args)]
pub(super) struct RecordWriteArgs {
    /// Domain whose records to modify (e.g. example.com).
    #[arg(value_name = "DOMAIN", value_parser = crate::idn::parse_name_arg)]
    pub(super) domain: String,

    // `ignore_case` is load-bearing, not cosmetic. The `required_if_eq` predicates
    // below compare against the *raw* `--type` value, not the `parse_write_type_arg`
    // output — so without it `--type tlsa` (accepted, and canonicalized to `TLSA`)
    // never trips the `--usage`/`--selector`/`--matching-type` requirement, and
    // `--type caa` never trips `--tag`.
    /// Record type (A, AAAA, ALIAS, CAA, CNAME, HTTPS, MX, SRV, SVCB, TLSA, TXT).
    #[arg(long = "type", value_name = "TYPE", value_parser = parse_write_type_arg, ignore_case = true)]
    pub(super) record_type: String,

    /// Record name relative to the domain (e.g. www, @ for the apex).
    #[arg(long, value_name = "NAME", value_parser = crate::idn::parse_name_arg)]
    pub(super) name: String,

    #[command(flatten)]
    pub(super) domain_format: crate::idn::DomainFormArgs,

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

    // A CAA record needs a tag; enforce it at parse time (before auth) — which
    // holds for `--type caa` as well as `--type CAA` only because `record_type`
    // sets `ignore_case`. The reverse guard — flag/tag only valid for CAA —
    // lives in the handler (`validate_caa_fields`), which clap can't express.
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
/// enforced by clap (`required_if_eq`, which covers a lower-case `--type` only
/// because the arg sets `ignore_case`); this only guards the reverse — they're
/// meaningless for other types. Pure so it's unit-testable and runs before any
/// network call.
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
/// [`v3_record`]'s TLSA branch). Delete/set per-record reporting and the
/// CNAME-conflict message need "the value" regardless of which wire field
/// holds it, so they read through this rather than `data` directly (exact-
/// duplicate/no-op detection compares full records via [`same_content`]
/// instead, since a shared value alone doesn't make two records identical).
/// Checks `type_` rather than just falling back on an absent `data`, so a
/// TLSA record correctly prefers `certificateData` even if the API ever
/// returns both fields populated.
pub(super) fn record_value(rec: &types::DnsRecord) -> Option<&str> {
    if rec.type_.as_str() == "TLSA" {
        rec.certificate_data.as_deref().or(rec.data.as_deref())
    } else {
        rec.data.as_deref().or(rec.certificate_data.as_deref())
    }
}

/// Re-merge a TLSA record's split fields (`usage`/`selector`/`matchingType`/
/// `certificateData`) back into a single presentation-format string, for
/// `dns list` output. DNS itself treats a TLSA record's RDATA as one blob
/// (RFC 6698 §2.1) — `dig`/zone-file output shows
/// `"<usage> <selector> <matching-type> <hex>"` — the v3 API's split into
/// four JSON fields is that API's own modeling, not something DNS does (same
/// story as CAA's `flag`/`tag`/`data` split). The write path (`v3_record`)
/// never populates `data` for TLSA, so `dns list` reconstructs it here for
/// display; the original four fields are left untouched alongside it. Only
/// builds the full presentation string when all three numeric fields are
/// present — the API's schema guarantees that together for a real TLSA
/// record, but substituting 0 (a real, meaningful usage/selector/matching-type
/// value, not a sentinel) for a genuinely absent field would fabricate RDATA
/// that was never actually returned; fall back to the certificate data alone.
pub(super) fn merged_tlsa_data(rec: &types::DnsRecord) -> String {
    match (&rec.usage, &rec.selector, &rec.matching_type) {
        (Some(usage), Some(selector), Some(matching_type)) => format!(
            "{} {} {} {}",
            usage.0,
            selector.0,
            matching_type.0,
            rec.certificate_data.as_deref().unwrap_or(""),
        ),
        _ => rec.certificate_data.clone().unwrap_or_default(),
    }
}

/// Whether two v3 records have the same content — every field except `ttl`
/// and `recordId`, neither of which makes two records a different *value*
/// (a TTL-only change was never treated as a new value, and `recordId` only
/// ever exists on a fetched record, never a desired one). Used by
/// duplicate/no-op detection so a record type whose identity spans several
/// fields (CAA's `flag`/`tag`, SRV's `priority`/`weight`/`port`, TLSA's
/// `usage`/`selector`/`matchingType`, HTTPS/SVCB's `priority`/`parameters`)
/// isn't misjudged by comparing only [`record_value`]. Compares via JSON
/// rather than a hand-maintained field list, so a future `DnsRecord` field
/// is covered without another update here.
pub(super) fn same_content(a: &types::DnsRecord, b: &types::DnsRecord) -> bool {
    comparable(a, false) == comparable(b, false)
}

/// The content of a record as the API would hold it, for comparing two records
/// that are meant to be the same. Drops `recordId` and, unless `compare_ttl`,
/// `ttl`; the API stores record names lower-cased, and hostname-valued `data`
/// lower-cased with a trailing dot (`Example.COM` comes back as `example.com.`),
/// so those are compared in that form. CAA `data` (the API lower-cases it, URLs
/// included) and TLSA `certificateData` (hex) are compared without case. Everything
/// else is compared exactly — the API keeps TXT text, AAAA addresses, HTTPS/SVCB
/// `parameters` and SRV `service`/`protocol` as written, so case in them is real.
///
/// Compares via JSON rather than a hand-maintained field list, so a future
/// `DnsRecord` field is covered without another update here. `DnsRecord` is all
/// plain String/Option/newtype fields, so serializing it should never fail —
/// falling back to a placeholder on error would let two unrelated records that
/// both failed to serialize compare as "equal" and silently skip a real change.
pub(super) fn comparable(record: &types::DnsRecord, compare_ttl: bool) -> Value {
    let mut value = serde_json::to_value(record).expect("DnsRecord always serializes to JSON");
    let record_type = record.type_.as_str();
    if let Some(fields) = value.as_object_mut() {
        fields.remove("recordId");
        if !compare_ttl {
            fields.remove("ttl");
        }
        if let Some(Value::String(name)) = fields.get_mut("name") {
            *name = name.to_lowercase();
        }
        if let Some(Value::String(data)) = fields.get_mut("data") {
            match record_type {
                "ALIAS" | "CNAME" | "MX" | "SRV" | "HTTPS" | "SVCB" => {
                    *data = data.trim_end_matches('.').to_lowercase();
                }
                "CAA" => *data = data.to_lowercase(),
                _ => {}
            }
        }
        // Hex digits mean the same in either case.
        if let Some(Value::String(cert)) = fields.get_mut("certificateData") {
            *cert = cert.to_lowercase();
        }
    }
    value
}

/// Types the API stores under a composed name (`_service._proto.name` for SRV,
/// `_port._proto.name` for TLSA), while a list response shows only the short
/// `name` plus separate fields. A server-side `name` filter for those types
/// matches the composed form, so filtering by the short name finds nothing.
const COMPOSITE_NAME_TYPES: &[&str] = &["SRV", "TLSA"];

/// List every v3 DNS record for a zone matching the optional `type`/`name`
/// filters, paging through the collection (v3 list is paginated). Shared by
/// `list`, `set`, and `delete` — the latter two need the matching records' ids.
///
/// For SRV and TLSA the `name` is matched here, against each record's short
/// name, rather than sent to the API: its filter only understands the composed
/// name, so it would return nothing for the name the record is listed under.
pub(super) async fn fetch_records(
    client: &domains_client::Client,
    zone: &str,
    type_: Option<&str>,
    name: Option<&str>,
    debug: bool,
) -> Result<Vec<types::DnsRecord>, CliCoreError> {
    let match_name_here = type_.is_some_and(|t| COMPOSITE_NAME_TYPES.contains(&t));
    let server_name = if match_name_here { None } else { name };
    let mut all = Vec::new();
    let mut page: u64 = 1;
    loop {
        let page_nz = std::num::NonZeroU64::new(page).expect("page starts at 1 and only grows");
        let mut req = client
            .list_dns_records()
            .zone(zone)
            .page(page_nz)
            .page_size(LIST_PAGE_SIZE)
            .total_required(true);
        if let Some(t) = type_ {
            req = req.type_(types::DnsRecordType(t.to_owned()));
        }
        if let Some(n) = server_name {
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
            .unwrap_or(got < LIST_PAGE_SIZE.get() as usize);
        if last || got == 0 {
            break;
        }
        page += 1;
    }
    if match_name_here && let Some(wanted) = name {
        // A record listed under exactly this name wins. Otherwise `chat` and a
        // separate record named `_sip._tcp.chat` would both answer to
        // `_sip._tcp.chat`, and a delete would reach an owner it never named.
        if all
            .iter()
            .any(|record| record.name.eq_ignore_ascii_case(wanted))
        {
            all.retain(|record| record.name.eq_ignore_ascii_case(wanted));
        } else {
            all.retain(|record| listed_name_matches(record, wanted));
        }
    }
    Ok(all)
}

/// Whether `wanted` names `record`: either the short name the record is listed
/// under, or the composed name the API itself understands (`_service._proto.name`
/// for SRV, `_port._proto.name` for TLSA), so a selector written either way finds
/// it. Case does not matter.
fn listed_name_matches(record: &types::DnsRecord, wanted: &str) -> bool {
    if record.name.eq_ignore_ascii_case(wanted) {
        return true;
    }
    let prefix = match record.type_.as_str() {
        "SRV" => record
            .service
            .as_deref()
            .zip(record.protocol.as_deref())
            .map(|(service, protocol)| format!("{service}.{protocol}")),
        "TLSA" => record
            .port
            .zip(record.protocol.as_deref())
            .map(|(port, protocol)| format!("_{port}.{protocol}")),
        _ => None,
    };
    // At the zone apex (`@`) the composed name is the prefix alone.
    let composed = prefix.map(|prefix| match record.name.as_str() {
        "@" => prefix,
        name => format!("{prefix}.{name}"),
    });
    composed.is_some_and(|composed| composed.eq_ignore_ascii_case(wanted))
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

/// A mutation's success payload plus the "verify with `dns list`" next action,
/// with the zone and record name shown in `domain_format`.
pub(super) fn verified_result(
    domain_format: DomainForm,
    mut payload: Value,
    domain: &str,
    record_type: &str,
    name: &str,
) -> CommandResult {
    domain_format.convert_fields(&mut payload, ZONE_AND_NAME);
    CommandResult::new(payload).with_next_actions(vec![verify_with_list_action(
        &domain_format.render(domain),
        record_type,
        &domain_format.render(name),
    )])
}

#[cfg(test)]
#[path = "records_tests.rs"]
mod tests;
