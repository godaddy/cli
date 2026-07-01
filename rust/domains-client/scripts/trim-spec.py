#!/usr/bin/env python3
"""Trim the upstream GoDaddy Domains Swagger 2.0 spec to the **retained v1**
subset the domains-client build still needs after the v3 migration.

Invoked by `regenerate-spec.sh` as:

    python3 trim-spec.py <upstream-swagger.json> <trimmed-output.json>

v3 (the Domain Lifecycle Management API) now serves availability, suggestions,
domain get, registration (quote → register), and single DNS-record creation.
The operations v3 does NOT yet cover stay on v1 and are the only ones kept here:

  * `GET /v1/domains`                       — list the shopper's domains
  * `GET /v1/domains/agreements`            — legal agreements for a TLD
  * `GET  /v1/domains/{domain}/records*`    — list DNS records (all/by-type/by-name)
  * `PUT  /v1/domains/{domain}/records/{type}/{name}` — replace a record set (dns set)
  * `DELETE /v1/domains/{domain}/records/{type}/{name}` — delete a record set (dns delete)

Everything kept here is a *read* or a record mutation; the transitive closure of
definitions they reference is kept too. The output is still Swagger 2.0; the
shell script then converts it to OpenAPI 3.0 and `merge-spec.py` folds it into
the v3 spec (prefixing these v1 definitions with `V1` to avoid name clashes).
"""

import copy
import json
import sys

# GET-only operations kept as-is (operationId pinned). `list` retrieves the
# domains owned by the authenticated shopper; `agreements` retrieves the legal
# agreements a TLD requires (upstream operationId `getAgreement`, pinned here to
# `agreements` so the generated builder reads `client.agreements()`).
AVAIL_OPS = {
    "/v1/domains": "list",
    "/v1/domains/agreements": "agreements",
}

# DNS record operations. v3 only creates single records, so the read (list) and
# the type+name replace/delete stay on v1. We keep every documented method on
# these paths and synthesize the GET-all / GET-by-type reads below.
RECORD_PATHS = [
    "/v1/domains/{domain}/records",
    "/v1/domains/{domain}/records/{type}",
    "/v1/domains/{domain}/records/{type}/{name}",
]

# String `format`s that typify maps to *external* crates (date-time/date ->
# chrono, uuid -> uuid). We don't depend on those (and deliberately keep this
# crate's dependency stack lean), and the CLI only passes these values straight
# through to JSON output, so drop the format and let them generate as plain
# `String` (the RFC 3339 / UUID text is unchanged on the wire).
EXTERNAL_FORMATS = {"date", "date-time", "uuid", "partial-date-time"}

# Read-only response definitions the CLI only deserializes-and-reprints. The
# published spec over-promises here: it marks fields `required` that the live API
# routinely omits (e.g. DomainSummary.contactRegistrant / renewDeadline on
# cancelled/pending domains) and types `nameServers` as a non-null array while
# the API returns JSON `null`. Relax these to tolerant readers.
#
# The complement — types the CLI *constructs* (record request bodies) — must stay
# strict, or call sites break. Everything not listed here is relaxed.
# NB: these are the *spec* definition names (upper-case `DNS…`), not the
# camel-cased Rust type names progenitor emits (`DnsRecord`).
STRICT_DEFS = {
    "DNSRecordCreateTypeName",  # built by `dns set`
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

    # Reads kept verbatim (GET only): list + agreements.
    for p, op_id in AVAIL_OPS.items():
        get = d["paths"][p]["get"]
        get["operationId"] = op_id
        get["produces"] = ["application/json"]      # drop xml/js variants
        paths[p] = {"get": only_2xx(get)}

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
            "title": "GoDaddy Domains API (retained v1 subset: list + agreements + DNS record list/set/delete)",
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
