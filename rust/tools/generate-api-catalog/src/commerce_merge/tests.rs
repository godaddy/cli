//! Tests for the commerce spec generator: the v2 transforms and the v1 merge.
//! Kept beside `commerce_merge.rs` rather than in it to stay under the
//! repository's per-file size limit.

use super::*;
use serde_json::json;

/// A v1 stores spec in the layout the pulled repo has (`v1/schemas/openapi.yaml`
/// beside `v2/schemas/openapi.yaml`), with the operation the client keeps, the
/// private and unrelated operations it must not, and a schema that nothing the
/// kept operation reaches uses.
const V1_FIXTURE: &str = r##"
openapi: 3.0.3
info: { title: Stores v1, version: "1.0" }
paths:
  /stores/{storeId}:
    parameters:
      - { name: storeId, in: path, required: true, schema: { type: string } }
      - name: includeField
        in: query
        explode: false
        style: form
        schema: { type: array, items: { type: string } }
    get:
      operationId: getStoreById
      responses:
        "200":
          description: Store Found
          content:
            application/json:
              schema: { $ref: "#/components/schemas/Store" }
            application/xml:
              schema: { $ref: "#/components/schemas/Store" }
        "404": { description: missing }
    patch:
      operationId: updateStore
      x-visibility: { extent: private }
      responses: { "200": { description: ok } }
  /businesses/{businessId}/stores:
    get:
      operationId: getStoresByBusinessId
      responses:
        "200":
          description: ok
          content:
            application/json:
              schema: { $ref: "#/components/schemas/Links" }
components:
  schemas:
    Store:
      type: object
      required: [displayName, businessId]
      properties:
        displayName: { type: string }
        processor: { $ref: "#/components/schemas/Processor" }
    Processor:
      type: object
      properties:
        name: { type: string }
    Links:
      type: object
      properties:
        next: { type: string }
"##;

/// Writes `<root>/v1/schemas/openapi.yaml` and returns the path of a sibling
/// `<root>/v2/schemas/openapi.yaml`, which is what the generator hands in.
fn repo_with_v1(root: &std::path::Path, v1: &str) -> std::path::PathBuf {
    let v1_dir = root.join("v1").join("schemas");
    std::fs::create_dir_all(&v1_dir).expect("create v1 dir");
    std::fs::write(v1_dir.join("openapi.yaml"), v1).expect("write v1 spec");
    let v2_dir = root.join("v2").join("schemas");
    std::fs::create_dir_all(&v2_dir).expect("create v2 dir");
    v2_dir.join("openapi.yaml")
}

fn v2_doc() -> Value {
    json!({
        "openapi": "3.0.3",
        "paths": { "/v2/commerce/stores/{storeId}": { "get": { "operationId": "getStoreById" } } },
        "components": { "schemas": { "Store": { "type": "object" } } }
    })
}

fn merged_v1() -> Value {
    let dir = tempfile::tempdir().expect("tempdir");
    let spec = repo_with_v1(dir.path(), V1_FIXTURE);
    let mut doc = v2_doc();
    merge_v1_get_store(&mut doc, &spec, None).expect("merges");
    doc
}

#[test]
fn only_the_v1_get_store_operation_is_merged_under_its_own_name_and_path() {
    let doc = merged_v1();
    let paths = doc["paths"].as_object().expect("paths object");
    let mut keys: Vec<&str> = paths.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "/v1/commerce/stores/{storeId}",
            "/v2/commerce/stores/{storeId}"
        ],
        "a private v1 operation or an unrelated one leaked in"
    );
    let item = &doc["paths"]["/v1/commerce/stores/{storeId}"];
    assert_eq!(item["get"]["operationId"], "getStoreByIdV1");
    assert!(
        item.get("patch").is_none(),
        "the private PATCH must not be merged"
    );
    assert_eq!(
        doc["paths"]["/v2/commerce/stores/{storeId}"]["get"]["operationId"], "getStoreById",
        "the v2 operation must be untouched"
    );
}

#[test]
fn the_v1_operation_keeps_its_parameters_and_only_its_json_response() {
    let doc = merged_v1();
    let item = &doc["paths"]["/v1/commerce/stores/{storeId}"];
    let names: Vec<&str> = item["parameters"]
        .as_array()
        .expect("path-level parameters are kept")
        .iter()
        .filter_map(|p| p["name"].as_str())
        .collect();
    assert_eq!(names, ["storeId", "includeField"]);
    let content = item["get"]["responses"]["200"]["content"]
        .as_object()
        .expect("content object");
    assert_eq!(content.keys().collect::<Vec<_>>(), ["application/json"]);
}

#[test]
fn v1_schemas_are_prefixed_so_they_cannot_collide_with_v2s_and_every_ref_follows() {
    let doc = merged_v1();
    let schemas = doc["components"]["schemas"].as_object().expect("schemas");
    assert!(
        schemas.contains_key("V1Store"),
        "{:?}",
        schemas.keys().collect::<Vec<_>>()
    );
    assert!(
        schemas.contains_key("V1Processor"),
        "reachable from the store"
    );
    assert_eq!(
        schemas["Store"],
        json!({ "type": "object" }),
        "the v2 Store must be untouched"
    );
    assert_eq!(
        doc["paths"]["/v1/commerce/stores/{storeId}"]["get"]["responses"]["200"]["content"]["application/json"]
            ["schema"]["$ref"],
        "#/components/schemas/V1Store"
    );
    assert_eq!(
        schemas["V1Store"]["properties"]["processor"]["$ref"],
        "#/components/schemas/V1Processor"
    );
}

#[test]
fn schemas_the_kept_operation_does_not_reach_are_not_merged() {
    let doc = merged_v1();
    assert!(
        doc["components"]["schemas"].get("V1Links").is_none(),
        "V1Links is only used by an operation that is not kept"
    );
}

/// The v1 spec lists `businessId` as required on a store but never declares
/// the property, while a live v1 record does carry it. Without declaring it the
/// generated v1 store would have no `business_id`.
#[test]
fn a_required_but_undeclared_business_id_is_declared() {
    let doc = merged_v1();
    assert_eq!(
        doc["components"]["schemas"]["V1Store"]["properties"]["businessId"]["type"],
        "string"
    );
}

#[test]
fn a_business_id_the_spec_already_declares_is_left_as_it_is() {
    let fixed = V1_FIXTURE.replace(
        "        displayName: { type: string }\n",
        "        displayName: { type: string }\n        businessId: { type: string, description: from upstream }\n",
    );
    let dir = tempfile::tempdir().expect("tempdir");
    let spec = repo_with_v1(dir.path(), &fixed);
    let mut doc = v2_doc();
    merge_v1_get_store(&mut doc, &spec, None).expect("merges");
    assert_eq!(
        doc["components"]["schemas"]["V1Store"]["properties"]["businessId"]["description"],
        "from upstream"
    );
}

#[test]
fn the_v1_spec_is_found_next_to_the_version_directory_the_generator_chose() {
    let path =
        std::path::Path::new("/tmp/clone/commerce.stores-specification/v2/schemas/openapi.yaml");
    assert_eq!(
        v1_spec_path(path).expect("derives"),
        std::path::PathBuf::from(
            "/tmp/clone/commerce.stores-specification/v1/schemas/openapi.yaml"
        )
    );
    assert!(v1_spec_path(std::path::Path::new("openapi.yaml")).is_err());
}

#[test]
fn a_missing_v1_spec_is_an_error_naming_it_not_a_silent_skip() {
    let dir = tempfile::tempdir().expect("tempdir");
    let v2_dir = dir.path().join("v2").join("schemas");
    std::fs::create_dir_all(&v2_dir).expect("create dir");
    let mut doc = v2_doc();
    let error = merge_v1_get_store(&mut doc, &v2_dir.join("openapi.yaml"), None)
        .expect_err("no v1 spec is an error");
    assert!(format!("{error:#}").contains("v1"), "{error:#}");
    assert_eq!(
        doc,
        v2_doc(),
        "a failed merge must leave the document alone"
    );
}

/// Merging v1 must not change v2: a v2 schema that no v2 path references is
/// still there afterwards (the fixture's v2 `Store` is exactly that).
#[test]
fn the_v2_document_is_left_exactly_as_it_was() {
    let doc = merged_v1();
    let before = v2_doc();
    assert_eq!(
        doc["paths"]["/v2/commerce/stores/{storeId}"],
        before["paths"]["/v2/commerce/stores/{storeId}"]
    );
    assert_eq!(
        doc["components"]["schemas"]["Store"],
        before["components"]["schemas"]["Store"]
    );
}

#[test]
fn only_schemas_are_merged_not_v1s_other_component_sections() {
    let with_sections = V1_FIXTURE.replace(
        "components:\n  schemas:",
        "components:\n  parameters:\n    requestId: { name: Poynt-Request-Id, in: header, schema: { type: string } }\n  headers:\n    Location: { schema: { type: string } }\n  schemas:",
    );
    let dir = tempfile::tempdir().expect("tempdir");
    let spec = repo_with_v1(dir.path(), &with_sections);
    let mut doc = v2_doc();
    merge_v1_get_store(&mut doc, &spec, None).expect("merges");
    let components = doc["components"].as_object().expect("components");
    assert_eq!(
        components.keys().collect::<Vec<_>>(),
        ["schemas"],
        "another v1 component section leaked in"
    );
}

/// The dereferencer already inlines a local ref into a non-schema component
/// section (a shared header, say), so the v1 operation arrives self-contained
/// and nothing dangles even though only schemas are merged.
#[test]
fn a_shared_header_is_inlined_and_does_not_leave_a_dangling_ref() {
    let shared = V1_FIXTURE
        .replace(
            "          description: Store Found\n",
            "          description: Store Found\n          headers:\n            Location: { $ref: \"#/components/headers/Location\" }\n",
        )
        .replace(
            "components:\n  schemas:",
            "components:\n  headers:\n    Location: { schema: { type: string } }\n  schemas:",
        );
    let dir = tempfile::tempdir().expect("tempdir");
    let spec = repo_with_v1(dir.path(), &shared);
    let mut doc = v2_doc();
    merge_v1_get_store(&mut doc, &spec, None).expect("merges");
    let response = &doc["paths"]["/v1/commerce/stores/{storeId}"]["get"]["responses"]["200"];
    assert_eq!(response["headers"]["Location"]["schema"]["type"], "string");
    assert!(
        !doc.to_string().contains("#/components/headers"),
        "a ref into a section that is not merged was left behind"
    );
}

/// Backstop for the case above: if a ref into another component section ever
/// does survive, merging only schemas would leave it dangling, so say so.
#[test]
fn a_reference_into_an_unmerged_component_section_is_reported() {
    let dangling = json!({ "get": { "responses": { "200": {
        "headers": { "Location": { "$ref": "#/components/headers/Location" } }
    } } } });
    let error = ensure_only_schema_refs(&dangling).expect_err("would dangle");
    assert!(
        error.to_string().contains("#/components/headers/Location"),
        "{error}"
    );
    let fine = json!({ "schema": { "$ref": "#/components/schemas/Store" } });
    assert!(ensure_only_schema_refs(&fine).is_ok());
}

/// v1 is only ever read, so its schemas are tolerant readers: no `required`
/// list to fail a sparse record on, and an array property may be `null`.
#[test]
fn v1_schemas_are_relaxed_readers() {
    let with_array = V1_FIXTURE.replace(
        "        displayName: { type: string }\n",
        "        displayName: { type: string }\n        governmentIds: { type: array, items: { type: string } }\n",
    );
    let dir = tempfile::tempdir().expect("tempdir");
    let spec = repo_with_v1(dir.path(), &with_array);
    let mut doc = v2_doc();
    merge_v1_get_store(&mut doc, &spec, None).expect("merges");
    let store = &doc["components"]["schemas"]["V1Store"];
    assert!(
        store.get("required").is_none(),
        "a v1 schema is still strict: {store}"
    );
    assert_eq!(store["properties"]["governmentIds"]["nullable"], true);
}

/// A reference to a schema the retained subset does not define would make the
/// document unusable to `progenitor`, and `ensure_refs_resolved` only looks at
/// references that leave the document, so it must be caught here, and the
/// document must be left as it was.
#[test]
fn a_reference_to_a_schema_that_is_not_defined_is_an_error_and_leaves_the_document_alone() {
    let dangling = V1_FIXTURE.replace(
        "        processor: { $ref: \"#/components/schemas/Processor\" }",
        "        processor: { $ref: \"#/components/schemas/MissingProcessor\" }",
    );
    assert_ne!(dangling, V1_FIXTURE, "the fixture edit must apply");
    let dir = tempfile::tempdir().expect("tempdir");
    let spec = repo_with_v1(dir.path(), &dangling);
    let mut doc = v2_doc();

    let error = merge_v1_get_store(&mut doc, &spec, None).expect_err("a dangling ref");
    assert!(
        format!("{error:#}").contains("MissingProcessor"),
        "the error should name the missing schema: {error:#}"
    );
    assert_eq!(
        doc,
        v2_doc(),
        "a failed merge must leave the document alone"
    );
}

#[test]
fn every_local_schema_reference_in_a_good_merge_resolves() {
    let doc = merged_v1();
    assert!(
        unresolved_local_schema_refs(&doc).is_empty(),
        "{:?}",
        unresolved_local_schema_refs(&doc)
    );
}

#[test]
fn the_check_finds_a_dangling_reference_anywhere_in_the_document() {
    let doc = json!({
        "paths": { "/a": { "get": { "responses": { "200": { "content": { "application/json": {
            "schema": { "$ref": "#/components/schemas/Gone" } } } } } } } },
        "components": { "schemas": {
            "Here": { "properties": { "x": { "$ref": "#/components/schemas/AlsoGone" } } }
        } }
    });
    assert_eq!(
        unresolved_local_schema_refs(&doc),
        ["AlsoGone".to_owned(), "Gone".to_owned()]
    );
}

/// progenitor turns a property `default` into a value it fills in when the
/// field is absent, so a store the API sent without a `status` would read as
/// `ACTIVE`, indistinguishable from one that was. The field stays absent.
#[test]
fn a_status_default_is_removed_so_an_absent_status_stays_absent() {
    let with_default = V1_FIXTURE.replace(
        "        displayName: { type: string }\n",
        "        displayName: { type: string }\n        status: { type: string, enum: [ACTIVE, DISABLED], default: ACTIVE }\n",
    );
    let dir = tempfile::tempdir().expect("tempdir");
    let spec = repo_with_v1(dir.path(), &with_default);
    let mut doc = v2_doc();
    merge_v1_get_store(&mut doc, &spec, None).expect("merges");
    let status = &doc["components"]["schemas"]["V1Store"]["properties"]["status"];
    assert!(status.get("default").is_none(), "{status}");
    assert_eq!(
        status["enum"],
        json!(["ACTIVE", "DISABLED"]),
        "the values stay"
    );
}

#[test]
fn property_defaults_are_removed_from_every_response_schema_at_any_depth() {
    let mut doc = json!({ "components": { "schemas": {
        "A": { "properties": {
            "status": { "type": "string", "default": "ACTIVE" },
            "nested": { "type": "object", "properties": {
                "kind": { "type": "string", "default": "x" } } },
            "items": { "type": "array", "items": { "type": "string", "default": "y" } }
        } }
    } } });
    strip_property_defaults(&mut doc);
    assert!(!doc.to_string().contains("default"), "{doc}");
}

/// A property literally named `default` is not a default value.
#[test]
fn a_property_named_default_is_not_mistaken_for_a_default_value() {
    let mut doc = json!({ "components": { "schemas": { "A": { "properties": {
        "default": { "type": "boolean" }
    } } } } });
    strip_property_defaults(&mut doc);
    assert_eq!(
        doc["components"]["schemas"]["A"]["properties"]["default"],
        json!({ "type": "boolean" })
    );
}

#[test]
fn a_v1_spec_without_the_operation_is_an_error() {
    let without = V1_FIXTURE.replace("/stores/{storeId}:", "/stores/{id}:");
    let dir = tempfile::tempdir().expect("tempdir");
    let spec = repo_with_v1(dir.path(), &without);
    let mut doc = v2_doc();
    let error = merge_v1_get_store(&mut doc, &spec, None).expect_err("operation is gone");
    assert!(
        format!("{error:#}").contains("/stores/{storeId}"),
        "{error:#}"
    );
}

#[test]
fn merging_over_an_existing_v1_path_or_schema_is_an_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    let spec = repo_with_v1(dir.path(), V1_FIXTURE);

    let mut path_taken = v2_doc();
    path_taken["paths"]["/v1/commerce/stores/{storeId}"] = json!({});
    assert!(merge_v1_get_store(&mut path_taken, &spec, None).is_err());

    let mut schema_taken = v2_doc();
    schema_taken["components"]["schemas"]["V1Store"] = json!({});
    assert!(merge_v1_get_store(&mut schema_taken, &spec, None).is_err());
}

/// A schema is strict if a request body reaches it directly or through
/// nested refs, including through an array and through a cycle; a schema
/// only a response reaches is not.
#[test]
fn request_reachable_schemas_follow_refs_and_ignore_response_only_ones() {
    let spec = json!({
        "paths": {
            "/stores/{id}/attributes/{name}": {
                "put": {
                    "requestBody": { "content": { "application/json": {
                        "schema": { "$ref": "#/components/schemas/AttributeUpdate" }
                    } } },
                    "responses": { "200": { "content": { "application/json": {
                        "schema": { "$ref": "#/components/schemas/Store" }
                    } } } }
                }
            },
            "/stores": {
                "get": { "responses": { "200": { "content": { "application/json": {
                    "schema": { "$ref": "#/components/schemas/StoreList" }
                } } } } }
            }
        },
        "components": { "schemas": {
            "AttributeUpdate": { "properties": {
                "value": { "$ref": "#/components/schemas/Value" },
                "tags": { "type": "array", "items": { "$ref": "#/components/schemas/Tag" } }
            } },
            "Value": { "properties": { "self": { "$ref": "#/components/schemas/Value" } } },
            "Tag": { "type": "object" },
            "Store": { "type": "object" },
            "StoreList": { "properties": { "items": { "$ref": "#/components/schemas/Store" } } }
        } }
    });

    let strict = request_reachable_schemas(&spec);

    for name in ["AttributeUpdate", "Value", "Tag"] {
        assert!(strict.contains(name), "{name} should be strict: {strict:?}");
    }
    for name in ["Store", "StoreList"] {
        assert!(
            !strict.contains(name),
            "{name} is response-only: {strict:?}"
        );
    }
}

#[test]
fn an_unresolved_external_ref_stops_the_refresh() {
    let unresolved = json!({
        "paths": { "/stores": { "get": { "responses": { "200": {
            "content": { "application/json": {
                "schema": { "$ref": "../common-types/store.yaml#/Store" }
            } }
        } } } } }
    });
    let error = ensure_refs_resolved(&unresolved).expect_err("an external ref is unresolved");
    assert!(
        error
            .to_string()
            .contains("../common-types/store.yaml#/Store"),
        "the error should name the ref: {error}"
    );

    let resolved = json!({
        "paths": { "/stores": { "get": { "responses": { "200": {
            "content": { "application/json": {
                "schema": { "$ref": "#/components/schemas/Store" }
            } }
        } } } } }
    });
    assert!(ensure_refs_resolved(&resolved).is_ok());
}

#[test]
fn only_unexploded_query_arrays_become_strings() {
    let mut spec = json!({
        "components": { "parameters": {
            "includeFields": {
                "name": "includeFields", "in": "query", "explode": false, "style": "form",
                "schema": { "type": "array", "items": { "type": "string" } }
            }
        } },
        "paths": { "/stores": { "get": { "parameters": [
            {
                "name": "ids", "in": "query", "explode": true,
                "schema": { "type": "array", "items": { "type": "string" } }
            },
            {
                "name": "tags", "in": "query",
                "schema": { "type": "array", "items": { "type": "string" } }
            },
            {
                "name": "sort", "in": "query", "explode": false,
                "schema": { "type": "string" }
            },
            {
                "name": "fields", "in": "header", "explode": false,
                "schema": { "type": "array", "items": { "type": "string" } }
            }
        ] } } }
    });

    flatten_unexploded_query_arrays(&mut spec);

    assert_eq!(
        spec["components"]["parameters"]["includeFields"]["schema"],
        json!({ "type": "string" })
    );
    let params = &spec["paths"]["/stores"]["get"]["parameters"];
    for (index, name) in [(0, "ids"), (1, "tags"), (3, "fields")] {
        assert_eq!(
            params[index]["schema"]["type"], "array",
            "{name} should stay an array"
        );
    }
    assert_eq!(params[2]["schema"], json!({ "type": "string" }));
}

#[test]
fn a_spec_with_no_request_bodies_has_no_strict_schemas() {
    let spec = json!({
        "paths": { "/stores": { "get": { "responses": { "200": {} } } } },
        "components": { "schemas": { "Store": { "required": ["id"] } } }
    });
    assert!(request_reachable_schemas(&spec).is_empty());
}

#[test]
fn relax_drops_required_and_makes_only_array_properties_nullable() {
    let mut schema = json!({
        "required": ["id"],
        "properties": {
            "tags": { "type": "array", "items": { "type": "string" } },
            "name": { "type": "string" }
        }
    });
    relax(&mut schema);
    assert!(schema.get("required").is_none());
    assert_eq!(schema["properties"]["tags"]["nullable"], true);
    assert!(schema["properties"]["name"].get("nullable").is_none());
}
