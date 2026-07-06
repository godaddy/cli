#!/usr/bin/env python3
"""Merge the retained-v1 OpenAPI 3.0 spec into the vendored v3 spec, producing
the single `domains.oas3.json` the crate's build.rs feeds to progenitor.

Invoked by `regenerate-spec.sh` as:

    python3 merge-spec.py <v1.oas3.json> <v3.yaml> <out.json> <host-base-url>

The CLI's `domain`/`dns` commands talk to two API generations at once: v3 (the
Domain Lifecycle Management API) for availability/suggest/get/quote/register and
the full DNS record lifecycle, and v1 for the handful of operations v3 does not
yet serve (list, agreements). We generate ONE progenitor client over ONE host
base URL by merging both into a single OAS3 document:

  * **v3 is the base.** Its server is `https://api.{env}.com/v3/domains`, so we
    rewrite every v3 path to the absolute `/v3/domains/...` form and pin the
    document `servers` to the bare host. One `base_url` (the host) then serves
    both `/v3/domains/...` and `/v1/domains/...` requests.
  * **v1 is injected with a `V1` name prefix.** v1 and v3 both define `DNSRecord`,
    `DnsRecordType`, `DomainStatus`, … with different shapes, so every v1
    component is renamed `V1<Name>` (and all its `$ref`s rewritten) before the
    merge — v3 keeps the clean, go-forward names.

Both sides are reduced to their 2xx responses (errors surface via HTTP status +
body, read by the CLI's `api_error`), and `date-time`/`uuid` string formats are
dropped so the generated crate needs neither chrono nor uuid.
"""

import json
import sys

import yaml

EXTERNAL_FORMATS = {"date", "date-time", "uuid", "partial-date-time"}

# v3 schemas the CLI *constructs* (request bodies) or that are dual-use
# (request + response): keep them strict so required fields stay required at the
# call site. Everything else in v3 components.schemas is a pure read and gets
# relaxed (required dropped, arrays made nullable) to tolerate sparse payloads.
STRICT_V3 = {
    "AvailabilityCheckCriteria",  # POST /check-availability body
    "Registration",               # POST /registrations body (and GET response)
    "Consent",
    "ConsentActor",
    "Contact",
    "Contacts",
    "simple-address",
    "InlineRegistrationProfile",
    "DNSRecord",                  # POST /zones/{zone}/dns-records body
    "NameServers",                # PUT .../nameservers body
    "NameserverHostname",
}


def strip_external_formats(obj):
    """Drop string `format`s that progenitor maps to external crates (chrono/uuid)
    and string `pattern`s that make it emit `regress::Regex` validation. The crate
    keeps its dependency stack lean (no chrono/uuid/regress); these values pass
    straight through as `String`, and the API validates them server-side (the CLI
    surfaces any 422 field errors). Mirrors the upstream-v1 stripping in trim-spec.py."""
    if isinstance(obj, dict):
        if obj.get("type") == "string":
            if obj.get("format") in EXTERNAL_FORMATS:
                obj.pop("format")
            # Drop string validation so progenitor emits plain `String` (or a thin
            # alias for named schemas) instead of a validated newtype per field —
            # the API validates these server-side and the CLI passes them through.
            obj.pop("pattern", None)
            obj.pop("minLength", None)
            obj.pop("maxLength", None)
        for v in obj.values():
            strip_external_formats(v)
    elif isinstance(obj, list):
        for v in obj:
            strip_external_formats(v)


def only_2xx_paths(paths):
    for item in paths.values():
        if not isinstance(item, dict):
            continue
        for method, op in item.items():
            if method not in ("get", "post", "put", "patch", "delete"):
                continue
            if isinstance(op, dict) and "responses" in op:
                op["responses"] = {
                    code: r
                    for code, r in op["responses"].items()
                    if str(code).startswith("2")
                }


def relax_oas3(defn):
    """Make an OAS3 response schema tolerant: drop `required` so each property is
    optional, and mark array properties `nullable` so an explicit JSON `null`
    deserializes to `None` rather than erroring."""
    if not isinstance(defn, dict):
        return
    defn.pop("required", None)
    for pschema in (defn.get("properties") or {}).values():
        if isinstance(pschema, dict) and pschema.get("type") == "array":
            pschema["nullable"] = True


def schema_refs(obj):
    """Collect the set of `#/components/schemas/<name>` targets referenced in obj."""
    out = set()
    if isinstance(obj, dict):
        ref = obj.get("$ref")
        if isinstance(ref, str) and ref.startswith("#/components/schemas/"):
            out.add(ref.split("/")[-1])
        for v in obj.values():
            out |= schema_refs(v)
    elif isinstance(obj, list):
        for v in obj:
            out |= schema_refs(v)
    return out


def prune_unreferenced_schemas(doc):
    """Drop component schemas unreachable from the (2xx-stripped) paths and the
    other component sections. After stripping error responses this removes the
    orphaned error-envelope model whose sanitized Rust name (`Error`) would
    otherwise collide with the common `error` schema and make progenitor emit a
    self-referential `struct Error(pub Error)`."""
    schemas = doc.get("components", {}).get("schemas", {})
    seed = schema_refs(doc.get("paths", {}))
    for section, members in doc.get("components", {}).items():
        if section != "schemas":
            seed |= schema_refs(members)
    reachable, stack = set(), list(seed)
    while stack:
        n = stack.pop()
        if n in reachable or n not in schemas:
            continue
        reachable.add(n)
        stack += list(schema_refs(schemas[n]))
    for name in [k for k in schemas if k not in reachable]:
        del schemas[name]


def rewrite_refs(obj, rename):
    """Rewrite every `#/components/<section>/<name>` $ref via `rename(section, name)`."""
    if isinstance(obj, dict):
        ref = obj.get("$ref")
        if isinstance(ref, str) and ref.startswith("#/components/"):
            parts = ref.split("/")
            if len(parts) == 4:
                _, _, section, name = parts
                obj["$ref"] = f"#/components/{section}/{rename(section, name)}"
        for v in obj.values():
            rewrite_refs(v, rename)
    elif isinstance(obj, list):
        for v in obj:
            rewrite_refs(v, rename)


def main(v1_path, v3_path, out_path, host):
    v3 = yaml.safe_load(open(v3_path))
    v1 = json.load(open(v1_path))

    # --- v3: rewrite paths to absolute /v3/domains/..., pin servers to the host.
    new_paths = {}
    for key, item in v3.get("paths", {}).items():
        if not key.startswith("/"):
            continue  # drop paths-level extensions (e.g. x-visibility)
        new_paths["/v3/domains" + key] = item
    v3["paths"] = new_paths
    v3["servers"] = [{"url": host, "description": "Domains API host"}]

    only_2xx_paths(v3["paths"])
    strip_external_formats(v3)

    # Relax pure-read v3 schemas; keep constructed/dual-use ones strict.
    for name, schema in (v3.get("components", {}).get("schemas") or {}).items():
        if name not in STRICT_V3:
            relax_oas3(schema)

    # `Registration` is dual-use: the register request requires `quoteToken`, but
    # the register/get *responses* never echo it. progenitor emits one Rust type
    # for both directions, so keeping `quoteToken` required makes response
    # deserialization fail ("missing field quoteToken"). Mark just that field
    # optional — the request handler always supplies it — while the other request
    # fields (domain/consent) stay required.
    registration = v3.get("components", {}).get("schemas", {}).get("Registration")
    if isinstance(registration, dict) and "required" in registration:
        registration["required"] = [
            field for field in registration["required"] if field != "quoteToken"
        ]

    # --- v1: prefix every component name with V1, rewrite its $refs, then merge.
    v1_components = v1.get("components", {})
    rename = lambda section, name: f"V1{name}"  # noqa: E731 — tiny local
    # Rewrite refs in v1 paths and v1 component bodies first (they reference the
    # OLD names; rewrite, then rename the keys).
    rewrite_refs(v1.get("paths", {}), rename)
    for section in v1_components.values():
        if isinstance(section, dict):
            rewrite_refs(section, rename)

    v3.setdefault("components", {})
    for section, members in v1_components.items():
        if not isinstance(members, dict):
            continue
        dst = v3["components"].setdefault(section, {})
        for name, body in members.items():
            dst[f"V1{name}"] = body

    only_2xx_paths(v1.get("paths", {}))
    # v1 paths (/v1/...) never collide with the rewritten v3 paths (/v3/domains/...).
    v3["paths"].update(v1.get("paths", {}))

    # Drop schemas left unreferenced once error responses are stripped, so no two
    # surviving schemas sanitize to the same Rust type name.
    prune_unreferenced_schemas(v3)

    json.dump(v3, open(out_path, "w"), indent=2)
    n_paths = len(v3["paths"])
    n_schemas = len(v3.get("components", {}).get("schemas", {}))
    print(f"   merged -> {out_path}: {n_paths} paths, {n_schemas} schemas")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4])
