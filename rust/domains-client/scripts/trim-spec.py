#!/usr/bin/env python3
"""Trim the upstream GoDaddy Domains Swagger 2.0 spec to the **retained v1**
subset the domains-client build still needs after the v3 migration.

Invoked by `regenerate-spec.sh` as:

    python3 trim-spec.py <upstream-swagger.json> <trimmed-output.json>

v3 (the Domain Lifecycle Management API) now serves availability, suggestions,
domain get, domain list (`listDomains`), registration (quote → register), and
the full DNS record lifecycle (create, list, replace, delete). The only
operation v3 does NOT yet cover stays on v1 and is the only one kept here:

  * `GET /v1/domains/agreements`            — legal agreements for a TLD

Everything kept here is a GET *read*; the transitive closure of definitions they
reference is kept too. The output is still Swagger 2.0; the shell script then
converts it to OpenAPI 3.0 and `merge-spec.py` folds it into the v3 spec
(prefixing these v1 definitions with `V1` to avoid name clashes).
"""

import json
import sys

# GET-only operations kept as-is (operationId pinned). `agreements` retrieves
# the legal agreements a TLD requires (upstream operationId `getAgreement`,
# pinned here to `agreements` so the generated builder reads
# `client.agreements()`). `GET /v1/domains` (list) was dropped once `domain
# list` migrated to v3's `listDomains` — nothing in the CLI calls v1's `list()`
# anymore.
AVAIL_OPS = {
    "/v1/domains/agreements": "agreements",
}

# String `format`s that typify maps to *external* crates (date-time/date ->
# chrono, uuid -> uuid). We don't depend on those (and deliberately keep this
# crate's dependency stack lean), and the CLI only passes these values straight
# through to JSON output, so drop the format and let them generate as plain
# `String` (the RFC 3339 / UUID text is unchanged on the wire).
EXTERNAL_FORMATS = {"date", "date-time", "uuid", "partial-date-time"}

# Read-only response definitions the CLI only deserializes-and-reprints. The
# published spec over-promises here in places: it marks fields `required` that
# the live API sometimes omits and types array fields as non-null while the
# API can return JSON `null`. Relax these to tolerant readers.
#
# The one retained v1 operation (agreements) is a GET read, so there are no
# request-body types to keep strict — everything is relaxed to a tolerant
# reader.
STRICT_DEFS: set[str] = set()


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

    # Reads kept verbatim (GET only): agreements.
    for p, op_id in AVAIL_OPS.items():
        get = d["paths"][p]["get"]
        get["operationId"] = op_id
        get["produces"] = ["application/json"]      # drop xml/js variants
        paths[p] = {"get": only_2xx(get)}

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
            "title": "GoDaddy Domains API (retained v1 subset: agreements)",
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
