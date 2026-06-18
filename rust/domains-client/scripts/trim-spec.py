#!/usr/bin/env python3
"""Trim the upstream GoDaddy Domains Swagger 2.0 spec to the subset the
domains-client build needs.

Invoked by `regenerate-spec.sh` as:

    python3 trim-spec.py <upstream-swagger.json> <trimmed-output.json>

Keeps the domains list + get + availability + suggest + agreements + purchase
(+ its per-TLD schema) + v2 register + DNS record operations and the transitive closure
of definitions they reference,
normalizes content types, and relaxes read-only response definitions (see the
inline comments). The output is still Swagger 2.0; the shell script then runs
`swagger2openapi` to produce the OpenAPI 3.0 spec progenitor consumes.
"""

import copy
import json
import sys

# GET-only operations kept as-is (operationId pinned). `list` retrieves the
# Domains owned by the authenticated shopper; available/suggest are the
# availability endpoints; agreements retrieves the legal agreements a TLD
# requires before purchase (upstream operationId `getAgreement`, pinned here to
# `agreements` so the generated builder reads `client.agreements()`).
AVAIL_OPS = {
    "/v1/domains": "list",
    "/v1/domains/available": "available",
    "/v1/domains/suggest": "suggest",
    "/v1/domains/agreements": "agreements",
}

# DNS record operations: keep every method these paths expose. The upstream
# Swagger documents `recordGet` only on the {type}/{name} path even though the
# gateway also routes GET on the bare and {type} paths (see the OAuth scope
# whitelist in gdcorp-domains/api-domain-data, api/oauthscopewhitelist.json);
# we synthesize those two GETs below so the CLI can list all / by-type records.
RECORD_PATHS = [
    "/v1/domains/{domain}/records",
    "/v1/domains/{domain}/records/{type}",
    "/v1/domains/{domain}/records/{type}/{name}",
]

# String `format`s that typify maps to *external* crates (date-time/date ->
# chrono, uuid -> uuid). We don't depend on those (and deliberately keep this
# crate's dependency stack lean — see Cargo.toml), and the CLI only passes these
# values straight through to JSON output, so drop the format and let them
# generate as plain `String` (the RFC 3339 / UUID text is unchanged on the wire).
EXTERNAL_FORMATS = {"date", "date-time", "uuid", "partial-date-time"}

# Read-only response definitions the CLI only deserializes-and-reprints. The
# published spec over-promises here: it marks fields `required` that the live API
# routinely omits (e.g. DomainSummary.contactRegistrant / renewDeadline on
# cancelled/pending domains) and types `nameServers` as a non-null array while
# the API returns JSON `null`. Relax these to tolerant readers.
#
# The complement — types the CLI *constructs* (request bodies) or reads via
# non-optional fields (e.g. `if available { … }`) — must stay strict, or call
# sites break. Everything not listed here is relaxed.
# NB: these are the *spec* definition names (upper-case `DNS…`), not the
# camel-cased Rust type names progenitor emits (`DnsRecord`).
STRICT_DEFS = {
    "DomainAvailableResponse",  # `available`/`suggest` read non-optional fields
    "DomainSuggestion",
    "DNSRecord",                # built by `dns add`
    "DNSRecordCreateType",
    "DNSRecordCreateTypeName",  # built by `dns set`
    "ArrayOfDNSRecord",         # `dns add` request body
    "DomainPurchase",           # `domain purchase` request body
    "Consent",                  # required sub-object of DomainPurchase
    "Contact",                  # optional contacts; keep required fields strict
    "Address",                  # contact mailing address (no required fields)
    "DomainPurchaseV2",         # v2 `domain purchase` request body (OAuth path)
    "ConsentV2",                # required sub-object of DomainPurchaseV2
    "DomainContactsCreateV2",   # v2 contacts wrapper
    "ContactDomainCreate",      # v2 per-role contact (reuses Address)
}


def only_2xx(op):
    # Keep only the 2xx response: the client needs the success type; non-2xx
    # surface via HTTP status + body. (Also avoids progenitor's single-error-type
    # constraint when the spec mixes Error + ErrorLimit across 4xx/5xx.)
    op["responses"] = {
        code: resp
        for code, resp in op.get("responses", {}).items()
        if str(code).startswith("2")
    }
    return op


def strip_external_formats(obj):
    if isinstance(obj, dict):
        if obj.get("type") == "string" and obj.get("format") in EXTERNAL_FORMATS:
            obj.pop("format")
        for v in obj.values():
            strip_external_formats(v)
    elif isinstance(obj, list):
        for v in obj:
            strip_external_formats(v)


def relax_definition(defn):
    # Drop `required` so every property generates as `Option` (missing -> None),
    # and mark array properties `x-nullable` so an explicit `null` deserializes to
    # `None` instead of erroring (`#[serde(default)]` alone only covers a *missing*
    # key, not a present-but-null one).
    if not isinstance(defn, dict):
        return
    defn.pop("required", None)
    for pschema in (defn.get("properties") or {}).values():
        if isinstance(pschema, dict) and pschema.get("type") == "array":
            pschema["x-nullable"] = True


def dedup_enums(obj):
    # typify cannot build a Rust enum from case-only duplicates
    # (e.g. ["FAST","FULL","fast","full"]); keep the first per case-fold.
    if isinstance(obj, dict):
        e = obj.get("enum")
        if isinstance(e, list):
            seen, uniq = set(), []
            for v in e:
                key = v.lower() if isinstance(v, str) else v
                if key not in seen:
                    seen.add(key)
                    uniq.append(v)
            obj["enum"] = uniq
        for v in obj.values():
            dedup_enums(v)
    elif isinstance(obj, list):
        for v in obj:
            dedup_enums(v)


def refs(obj):
    out = set()
    if isinstance(obj, dict):
        for k, v in obj.items():
            if k == "$ref" and isinstance(v, str):
                out.add(v.split("/")[-1])
            else:
                out |= refs(v)
    elif isinstance(obj, list):
        for v in obj:
            out |= refs(v)
    return out


def main(src, dst):
    d = json.load(open(src))

    paths = {}

    # Availability + suggest + agreements (GET only).
    for p, op_id in AVAIL_OPS.items():
        get = d["paths"][p]["get"]
        get["operationId"] = op_id
        get["produces"] = ["application/json"]      # drop xml/js variants
        paths[p] = {"get": only_2xx(get)}

    # Purchase (POST with JSON body). The body ($ref DomainPurchase) and its
    # transitive closure (Consent/Contact/Address) are kept strict below so the
    # generated request types keep their required fields.
    purchase = d["paths"]["/v1/domains/purchase"]["post"]
    purchase["operationId"] = "purchase"
    purchase["produces"] = ["application/json"]
    purchase["consumes"] = ["application/json"]
    paths["/v1/domains/purchase"] = {"post": only_2xx(purchase)}

    # Per-TLD purchase schema (GET). `domain purchase` reads its top-level
    # `required` array to preflight-validate requirements before the paid call.
    # The upstream JsonSchema/JsonProperty/JsonDataType definitions are loosely
    # typed (`"type": "object"` *with* `items`), so instead of pulling them into
    # the closure we replace the response with a free-form object — progenitor
    # then emits `serde_json::Value`, and we parse only the fields we need.
    schema_op = d["paths"]["/v1/domains/purchase/schema/{tld}"]["get"]
    schema_op["operationId"] = "schema"
    schema_op["produces"] = ["application/json"]
    only_2xx(schema_op)
    schema_op["responses"]["200"]["schema"] = {"type": "object"}
    paths["/v1/domains/purchase/schema/{tld}"] = {"get": schema_op}

    # Get one domain's details (GET). `DomainDetail` embeds the Contact/Address
    # types we keep strict for v2 request construction, but as a *read* the API
    # can return them sparsely (e.g. privacy-protected domains). Rather than fight
    # that strict/relax conflict, take the response as a free-form object —
    # progenitor emits `serde_json::Map` — and emit it directly (the command just
    # dumps the domain's details).
    get_op = d["paths"]["/v1/domains/{domain}"]["get"]
    get_op["operationId"] = "get"
    get_op["produces"] = ["application/json"]
    only_2xx(get_op)
    # `get` has two success responses (200 and 203), both DomainDetail; free-form
    # every kept 2xx so progenitor sees a single response type (and no DomainDetail
    # closure is pulled in).
    for resp in get_op["responses"].values():
        resp["schema"] = {"type": "object"}
    paths["/v1/domains/{domain}"] = {"get": get_op}

    # v2 register (POST). The OAuth-friendly purchase path: unlike v1 purchase,
    # v2 authorizes credit-card payments for OAuth bearer users. Body
    # ($ref DomainPurchaseV2) + closure (ConsentV2/DomainContactsCreateV2/
    # ContactDomainCreate, reusing Address) are kept strict below. Upstream has
    # no operationId, so pin `register`. The 2xx is a bodyless 202 (async).
    register = d["paths"]["/v2/customers/{customerId}/domains/register"]["post"]
    register["operationId"] = "register"
    register["produces"] = ["application/json"]
    register["consumes"] = ["application/json"]
    paths["/v2/customers/{customerId}/domains/register"] = {"post": only_2xx(register)}

    # DNS record ops: keep all documented methods, success responses only, and
    # normalize content types (drop xml/js variants; request bodies stay JSON).
    for p in RECORD_PATHS:
        kept = {}
        for method, op in d["paths"][p].items():
            if method not in ("get", "put", "post", "patch", "delete"):
                continue  # skip non-operation keys (e.g. shared "parameters")
            op["produces"] = ["application/json"]
            if any(prm.get("in") == "body" for prm in op.get("parameters", [])):
                op["consumes"] = ["application/json"]
            kept[method] = only_2xx(op)
        paths[p] = kept

    # Synthesize GET-all and GET-by-type from the documented recordGet operation:
    # same DNSRecord-array success response, fewer path params, no offset/limit.
    record_get = d["paths"]["/v1/domains/{domain}/records/{type}/{name}"]["get"]

    def synth_get(keep_param_names, op_id):
        op = copy.deepcopy(record_get)
        op["operationId"] = op_id
        op["produces"] = ["application/json"]
        # Keep the named path params plus any header params (e.g. the optional
        # X-Shopper-Id the record routes accept) so the synthesized ops mirror
        # what the route supports; drop the query params (offset/limit) that only
        # apply to the type+name GET.
        op["parameters"] = [
            prm
            for prm in op.get("parameters", [])
            if prm.get("in") == "header" or prm.get("name") in keep_param_names
        ]
        return only_2xx(op)

    paths["/v1/domains/{domain}/records"]["get"] = synth_get({"domain"}, "recordGetAll")
    paths["/v1/domains/{domain}/records/{type}"]["get"] = synth_get(
        {"domain", "type"}, "recordGetByType"
    )

    dedup_enums(paths)

    needed, stack = set(), list(refs(paths))
    while stack:
        n = stack.pop()
        if n in needed:
            continue
        needed.add(n)
        stack += [r for r in refs(d["definitions"].get(n, {})) if r not in needed]

    out = {
        "swagger": "2.0",
        "info": {
            "title": "GoDaddy Domains API (domains list + get + availability + agreements + purchase + v2 register + DNS records subset)",
            "version": d.get("info", {}).get("version", "1.0.0"),
        },
        "host": d.get("host", "api.ote-godaddy.com"),
        "basePath": d.get("basePath", "/"),
        "schemes": d.get("schemes", ["https"]),
        "paths": paths,
        "definitions": {
            n: d["definitions"][n] for n in sorted(needed) if n in d["definitions"]
        },
    }
    for name, defn in out["definitions"].items():
        if name not in STRICT_DEFS:
            relax_definition(defn)

    strip_external_formats(out)
    json.dump(out, open(dst, "w"), indent=2)
    print(f"   kept {len(paths)} paths, {len(out['definitions'])} definitions")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
