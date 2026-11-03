use super::*;
use std::fs;

#[test]
fn dereference_resolves_relative_file_ref() {
    let dir = tempfile::tempdir().expect("tempdir");
    let model_dir = dir.path().join("schemas").join("models");
    fs::create_dir_all(&model_dir).expect("create model dir");

    fs::write(
        model_dir.join("Widget.yaml"),
        "type: object\nproperties:\n  id:\n    type: string\n",
    )
    .expect("write model");

    let spec_path = dir.path().join("schemas").join("openapi.yaml");
    fs::write(
            &spec_path,
            "openapi: '3.0.0'\ncomponents:\n  schemas:\n    Widget:\n      $ref: './models/Widget.yaml'\n",
        )
        .expect("write spec");

    let (result, _defs) = load_and_dereference(&spec_path, None).expect("dereference");
    let unresolved = collect_unresolved_refs(&result);
    assert!(
        unresolved.is_empty(),
        "expected zero unresolved $refs, got: {unresolved:?}"
    );
}

#[test]
fn dereference_resolves_ref_in_sibling_of_schemas_dir() {
    // Models at {dir}/models/ rather than {dir}/schemas/models/ (sibling layout)
    let dir = tempfile::tempdir().expect("tempdir");
    let model_dir = dir.path().join("models");
    fs::create_dir_all(&model_dir).expect("create model dir");
    let schemas_dir = dir.path().join("schemas");
    fs::create_dir_all(&schemas_dir).expect("create schemas dir");

    fs::write(
        model_dir.join("Widget.yaml"),
        "type: object\nproperties:\n  id:\n    type: string\n",
    )
    .expect("write model");

    let spec_path = schemas_dir.join("openapi.yaml");
    fs::write(
            &spec_path,
            "openapi: '3.0.0'\ncomponents:\n  schemas:\n    Widget:\n      $ref: './models/Widget.yaml'\n",
        )
        .expect("write spec");

    let (result, _defs) = load_and_dereference(&spec_path, None).expect("dereference");
    let unresolved = collect_unresolved_refs(&result);
    assert!(
        unresolved.is_empty(),
        "expected zero unresolved $refs with sibling layout, got: {unresolved:?}"
    );
}

#[test]
fn collect_unresolved_refs_finds_external_refs() {
    let value = serde_json::json!({
        "components": {
            "schemas": {
                "Foo": { "$ref": "./models/Foo.yaml" },
                "Bar": { "$ref": "#/components/schemas/Baz" },
                "Qux": { "type": "string" }
            }
        }
    });
    let unresolved = collect_unresolved_refs(&value);
    assert_eq!(unresolved, vec!["./models/Foo.yaml"]);
}

#[test]
fn collect_unresolved_refs_finds_external_discriminator_mapping() {
    // A discriminator.mapping value pointing outside the document is unresolved,
    // even though it is a plain string rather than a `$ref` object.
    let value = serde_json::json!({
        "$defs": {
            "Channel": {
                "discriminator": {
                    "propertyName": "kind",
                    "mapping": {
                        "DEFAULT": "./BasicChannel.yaml",
                        "LOCAL": "#/$defs/LocalChannel"
                    }
                }
            }
        }
    });
    let unresolved = collect_unresolved_refs(&value);
    assert_eq!(unresolved, vec!["./BasicChannel.yaml"]);
}

#[test]
fn discriminator_mapping_rewritten_to_defs_pointer() {
    let dir = tempfile::tempdir().expect("tempdir");
    let schemas_dir = dir.path().join("schemas");
    fs::create_dir_all(&schemas_dir).expect("create schemas dir");

    fs::write(
        schemas_dir.join("BasicChannel.yaml"),
        "type: object\nproperties:\n  kind:\n    type: string\n",
    )
    .expect("write model");

    // The oneOf `$ref` lifts BasicChannel into `$defs`; the discriminator.mapping
    // carries the same target as a plain string that dereference() won't touch.
    let spec_path = schemas_dir.join("openapi.yaml");
    fs::write(
        &spec_path,
        concat!(
            "openapi: '3.0.0'\n",
            "components:\n",
            "  schemas:\n",
            "    Channel:\n",
            "      oneOf:\n",
            "        - $ref: './BasicChannel.yaml'\n",
            "      discriminator:\n",
            "        propertyName: kind\n",
            "        mapping:\n",
            "          DEFAULT: './BasicChannel.yaml'\n",
        ),
    )
    .expect("write spec");

    let (result, defs) = load_and_dereference(&spec_path, None).expect("dereference");

    // No external refs remain — the mapping value now points inside the document.
    let mut unresolved = collect_unresolved_refs(&result);
    for def_val in defs.values() {
        unresolved.extend(collect_unresolved_refs(def_val));
    }
    assert!(
        unresolved.is_empty(),
        "expected zero unresolved refs after mapping rewrite, got: {unresolved:?}"
    );

    // The mapping value is rewritten to the lifted def pointer.
    let mapping = result["components"]["schemas"]["Channel"]["discriminator"]["mapping"]["DEFAULT"]
        .as_str()
        .expect("mapping DEFAULT string");
    assert_eq!(mapping, "#/$defs/BasicChannel");
    assert!(defs.contains_key("BasicChannel"));
}

#[test]
fn discriminator_mapping_target_not_lifted_stays_external_and_fails_guard() {
    // Safety branch: a mapping value whose target is NOT otherwise referenced
    // (no oneOf/$ref lifts it into $defs) must be left external — never pointed
    // at a missing #/$defs key — so collect_unresolved_refs then flags it and
    // the build fails, rather than silently producing a dangling pointer.
    let dir = tempfile::tempdir().expect("tempdir");
    let schemas_dir = dir.path().join("schemas");
    fs::create_dir_all(&schemas_dir).expect("create schemas dir");

    // Note: no BasicChannel.yaml on disk and no oneOf/$ref to it — nothing lifts it.
    let spec_path = schemas_dir.join("openapi.yaml");
    fs::write(
        &spec_path,
        concat!(
            "openapi: '3.0.0'\n",
            "components:\n",
            "  schemas:\n",
            "    Channel:\n",
            "      type: object\n",
            "      discriminator:\n",
            "        propertyName: kind\n",
            "        mapping:\n",
            "          DEFAULT: './BasicChannel.yaml'\n",
        ),
    )
    .expect("write spec");

    let (result, defs) = load_and_dereference(&spec_path, None).expect("dereference");

    // The mapping value is left untouched (no matching def to point at)...
    let mapping = result["components"]["schemas"]["Channel"]["discriminator"]["mapping"]["DEFAULT"]
        .as_str()
        .expect("mapping DEFAULT string");
    assert_eq!(mapping, "./BasicChannel.yaml");
    assert!(!defs.contains_key("BasicChannel"));

    // ...and the unresolved-ref guard flags it, so the build would fail.
    let mut unresolved = collect_unresolved_refs(&result);
    for def_val in defs.values() {
        unresolved.extend(collect_unresolved_refs(def_val));
    }
    assert!(
        unresolved.contains(&"./BasicChannel.yaml".to_owned()),
        "expected the un-liftable mapping to be flagged as unresolved, got: {unresolved:?}"
    );
}

#[test]
fn dereference_lifts_external_ref_to_defs() {
    let dir = tempfile::tempdir().expect("tempdir");
    let model_dir = dir.path().join("schemas").join("models");
    fs::create_dir_all(&model_dir).expect("create model dir");

    fs::write(
        model_dir.join("Widget.yaml"),
        "type: object\nproperties:\n  id:\n    type: string\n",
    )
    .expect("write model");

    let spec_path = dir.path().join("schemas").join("openapi.yaml");
    fs::write(
            &spec_path,
            "openapi: '3.0.0'\ncomponents:\n  schemas:\n    Widget:\n      $ref: './models/Widget.yaml'\n",
        )
        .expect("write spec");

    let (_result, defs) = load_and_dereference(&spec_path, None).expect("dereference");
    assert!(
        defs.contains_key("Widget"),
        "Widget should be lifted to $defs"
    );
    let widget = &defs["Widget"];
    assert_eq!(widget.get("type").and_then(|v| v.as_str()), Some("object"));
}

#[test]
fn dereference_deduplicates_same_external_ref() {
    let dir = tempfile::tempdir().expect("tempdir");
    let model_dir = dir.path().join("schemas").join("models");
    fs::create_dir_all(&model_dir).expect("create model dir");

    fs::write(
        model_dir.join("Tag.yaml"),
        "type: object\nproperties:\n  name:\n    type: string\n",
    )
    .expect("write model");

    // Spec references Tag from two different endpoints
    let spec_path = dir.path().join("schemas").join("openapi.yaml");
    fs::write(
        &spec_path,
        r#"openapi: '3.0.0'
paths:
  /a:
    get:
      operationId: getA
      responses:
        '200':
          content:
            application/json:
              schema:
                $ref: './models/Tag.yaml'
  /b:
    get:
      operationId: getB
      responses:
        '200':
          content:
            application/json:
              schema:
                $ref: './models/Tag.yaml'
"#,
    )
    .expect("write spec");

    let (result, defs) = load_and_dereference(&spec_path, None).expect("dereference");
    assert!(defs.contains_key("Tag"), "Tag should be in $defs");

    // Both response schemas should be $ref pointers, not inlined
    let paths = result.get("paths").expect("paths");
    let schema_a = paths["/a"]["get"]["responses"]["200"]["content"]["application/json"]["schema"]
        .as_object()
        .expect("schema_a object");
    assert_eq!(
        schema_a.get("$ref").and_then(|v| v.as_str()),
        Some("#/$defs/Tag"),
        "schema_a should be a local $defs ref"
    );
    let schema_b = paths["/b"]["get"]["responses"]["200"]["content"]["application/json"]["schema"]
        .as_object()
        .expect("schema_b object");
    assert_eq!(
        schema_b.get("$ref").and_then(|v| v.as_str()),
        Some("#/$defs/Tag"),
        "schema_b should be a local $defs ref"
    );
}

#[test]
fn dereference_keeps_siblings_next_to_a_lifted_ref() {
    let dir = tempfile::tempdir().expect("tempdir");
    let spec_path = dir.path().join("openapi.yaml");
    fs::write(
        &spec_path,
        r#"openapi: '3.0.0'
components:
  schemas:
    Item:
      type: object
      properties:
        id:
          type: string
    Holder:
      type: object
      properties:
        first:
          $ref: '#/components/schemas/Item'
          description: The first item
          readOnly: true
        second:
          $ref: '#/components/schemas/Item'
          description: The second item
"#,
    )
    .expect("write spec");

    let (result, defs) = load_and_dereference(&spec_path, None).expect("dereference");
    let holder = &result["components"]["schemas"]["Holder"]["properties"];
    // First use lifts Item into $defs; second use is the already-registered path.
    assert_eq!(
        holder["first"],
        serde_json::json!({
            "$ref": "#/$defs/Item",
            "description": "The first item",
            "readOnly": true,
        })
    );
    assert_eq!(
        holder["second"],
        serde_json::json!({
            "$ref": "#/$defs/Item",
            "description": "The second item",
        })
    );
    assert!(
        defs["Item"].get("description").is_none(),
        "per-use siblings must not leak into the shared definition"
    );
}

#[test]
fn dereference_keeps_constraints_a_schema_file_adds_to_its_ref() {
    // Mirrors orders' positive-money: a file that `$ref`s a base schema and
    // tightens one of its properties.
    let dir = tempfile::tempdir().expect("tempdir");
    let model_dir = dir.path().join("schemas").join("models");
    fs::create_dir_all(&model_dir).expect("create model dir");
    fs::write(
        model_dir.join("money.json"),
        r#"{"type":"object","properties":{"value":{"type":"integer"}}}"#,
    )
    .expect("write base");
    fs::write(
        model_dir.join("positive-money.json"),
        r#"{"$ref":"./money.json","properties":{"value":{"minimum":0}}}"#,
    )
    .expect("write refinement");
    let spec_path = dir.path().join("schemas").join("openapi.yaml");
    fs::write(
            &spec_path,
            "openapi: '3.0.0'\ncomponents:\n  schemas:\n    Price:\n      $ref: './models/positive-money.json'\n",
        )
        .expect("write spec");

    let (_result, defs) = load_and_dereference(&spec_path, None).expect("dereference");
    assert_eq!(
        defs["positive-money"],
        serde_json::json!({
            "$ref": "#/$defs/money",
            "properties": { "value": { "minimum": 0 } },
        })
    );
    assert_eq!(defs["money"]["properties"]["value"]["type"], "integer");
}

#[test]
fn dereference_lifts_component_schema_ref() {
    let dir = tempfile::tempdir().expect("tempdir");
    let spec_path = dir.path().join("openapi.yaml");
    fs::write(
        &spec_path,
        r#"openapi: '3.0.0'
components:
  schemas:
    Item:
      type: object
      properties:
        id:
          type: string
paths:
  /items:
    get:
      operationId: listItems
      responses:
        '200':
          content:
            application/json:
              schema:
                type: array
                items:
                  $ref: '#/components/schemas/Item'
"#,
    )
    .expect("write spec");

    let (result, defs) = load_and_dereference(&spec_path, None).expect("dereference");

    // Item should be lifted to $defs
    assert!(defs.contains_key("Item"), "Item should be in $defs");

    // The response schema items should be a local $ref
    let schema = result["paths"]["/items"]["get"]["responses"]["200"]["content"]
            ["application/json"]["schema"]
            .as_object()
            .expect("schema object");
    let items_ref = schema["items"]
        .get("$ref")
        .and_then(|v| v.as_str())
        .expect("items.$ref");
    assert_eq!(items_ref, "#/$defs/Item");
}

#[test]
fn dereference_circular_ref_guard() {
    // Schema A references schema B which references schema A — must not infinite-loop
    let dir = tempfile::tempdir().expect("tempdir");
    let spec_path = dir.path().join("openapi.yaml");
    fs::write(
        &spec_path,
        r#"openapi: '3.0.0'
components:
  schemas:
    NodeA:
      type: object
      properties:
        child:
          $ref: '#/components/schemas/NodeB'
    NodeB:
      type: object
      properties:
        parent:
          $ref: '#/components/schemas/NodeA'
"#,
    )
    .expect("write spec");

    // Must complete without stack overflow or panic
    let (_result, defs) = load_and_dereference(&spec_path, None).expect("dereference");
    assert!(defs.contains_key("NodeA") || defs.contains_key("NodeB"));
}

#[test]
fn derive_defs_key_for_path_extracts_stem() {
    assert_eq!(derive_defs_key_for_path("./models/Order.yaml"), "Order");
    assert_eq!(
        derive_defs_key_for_path("../common-types/error.json"),
        "error"
    );
    assert_eq!(
        derive_defs_key_for_path("https://schemas.api.godaddy.com/v1/json/address.json"),
        "address"
    );
    assert_eq!(
        derive_defs_key_for_path("./Bulk_Ingestion.yaml"),
        "Bulk_Ingestion"
    );
    // Fragment with component name takes precedence over file stem
    assert_eq!(
        derive_defs_key_for_path("./common.yaml#/components/schemas/Foo"),
        "Foo"
    );
    assert_eq!(
        derive_defs_key_for_path("./shared.yaml#/definitions/Bar"),
        "Bar"
    );
    // A `#/properties/<name>` fragment picks out one property's schema, not the
    // file's root schema — it must be folded into the key so two refs into the
    // same file for different properties don't collide (DEVEX-965).
    assert_eq!(
        derive_defs_key_for_path("./models/FulfillmentPlan.yaml#/properties/storeId"),
        "FulfillmentPlan_storeId"
    );
    assert_eq!(
        derive_defs_key_for_path("./models/FulfillmentPlan.yaml#/properties/orderId"),
        "FulfillmentPlan_orderId"
    );
    assert_eq!(
        derive_defs_key_for_path("./models/Foo.yaml#/properties/a/properties/b"),
        "Foo_a_b"
    );
    // A literal property named `a_b` must not collide with nested segments `a`
    // then `b` — both would sanitize to the same "a_b" suffix without escaping
    // the literal underscore first.
    assert_ne!(
        derive_defs_key_for_path("./models/Foo.yaml#/properties/a_b"),
        derive_defs_key_for_path("./models/Foo.yaml#/properties/a/properties/b")
    );
}
