//! The contract: for every operation of the target API (`spec/openapi.json`,
//! the axum starter's document) that this application covers, the document
//! it serves matches the target's.
//!
//! Compared, for each covered operation: the method and path, the operation
//! id and tags, the parameters (name, `in`, required, schema), the request
//! body, and the success status and its response schema. Schemas are
//! compared structurally: their keywords (properties, types, formats,
//! nullability, required, enum values, bounds), recursively, with `$ref`s
//! resolved in each document. A `$ref` on either side needs a `$ref` to the
//! schema of the same name on the other: a schema inlined where the other
//! document names one is a difference. A `$ref` the target never defines
//! (`UserSort`, `SortOrder`) is compared by name only. Descriptions,
//! summaries, titles, examples and defaults are not compared, nor are extra
//! statuses, error bodies and extra operations the application serves.
//!
//! One deliberate allowance: a target success response without a body
//! documents none, so the application may serve any body there (no covered
//! operation relies on it today). A target body the application doesn't
//! serve is still a difference.
//!
//! [`KNOWN`] lists the differences the application still has, each with why.
//! The contract fails on any other difference, and on a known one that is
//! gone (so the list stays true).

use std::collections::{BTreeMap, BTreeSet};

use actix_api::app::configure;
use actix_api::shared::auth::JwtKeys;
use actix_web::{App, test, web};
use serde_json::Value;

/// The target document.
const TARGET: &str = include_str!("spec/openapi.json");

/// The target operations the application covers.
const COVERED: &[(&str, &str)] = &[
    ("get", "/api/v1/users"),
    ("get", "/api/v1/users/{id}"),
    ("patch", "/api/v1/users/{id}"),
    ("delete", "/api/v1/users/{id}"),
    ("get", "/api/v1/audit-log"),
    ("get", "/health/live"),
    ("get", "/health/ready"),
];

/// The differences the application still has, and why.
const KNOWN: &[(&str, &str)] = &[
    (
        "GET /api/v1/users: response 200: properties.items.items.properties: target has `avatar_url`, served has not",
        "`avatar_url` comes with uploads (a later release)",
    ),
    (
        "GET /api/v1/users/{id}: response 200: properties: target has `avatar_url`, served has not",
        "`avatar_url` comes with uploads (a later release)",
    ),
    (
        "PATCH /api/v1/users/{id}: response 200: properties: target has `avatar_url`, served has not",
        "`avatar_url` comes with uploads (a later release)",
    ),
    (
        "DELETE /api/v1/users/{id}: parameter `permanent` (query): served has it, target has not",
        "every generated soft-delete `DELETE` takes `?permanent=true` (ADR 0016)",
    ),
    (
        "GET /api/v1/audit-log: parameter `sort` (query): served has it, target has not",
        "every generated list takes `sort` (ADR 0016); the default is newest first, as the target's",
    ),
    (
        "GET /api/v1/audit-log: parameter `order` (query): served has it, target has not",
        "every generated list takes `order` (ADR 0016)",
    ),
];

/// The keywords that describe a schema rather than constrain it.
const ANNOTATIONS: &[&str] = &[
    "description",
    "summary",
    "title",
    "example",
    "examples",
    "default",
    "deprecated",
];

/// One of the two documents, for resolving its `$ref`s.
#[derive(Clone, Copy)]
struct Doc<'a> {
    root: &'a Value,
    name: &'static str,
}

impl<'a> Doc<'a> {
    /// The schema `#/components/schemas/<name>`, if the document defines it.
    fn schema(self, name: &str) -> Option<&'a Value> {
        self.root
            .get("components")
            .and_then(|components| components.get("schemas"))
            .and_then(|schemas| schemas.get(name))
    }
}

/// The schema name a `$ref` points at.
fn ref_name(schema: &Value) -> Option<&str> {
    schema
        .get("$ref")
        .and_then(Value::as_str)
        .map(|reference| reference.rsplit('/').next().unwrap_or(reference))
}

/// Compares two values, either of which may be a schema, a list of schemas,
/// or a keyword's value, and appends each difference found under `at`.
struct Compare<'a> {
    target: Doc<'a>,
    served: Doc<'a>,
    differences: Vec<String>,
}

impl Compare<'_> {
    fn differ(&mut self, at: &str, what: String) {
        if at.is_empty() || at.ends_with(": ") {
            self.differences.push(format!("{at}{what}"));
        } else {
            self.differences.push(format!("{at}: {what}"));
        }
    }

    /// Compares two schemas.
    fn schema(&mut self, at: &str, target: &Value, served: &Value) {
        let (target_ref, served_ref) = (ref_name(target), ref_name(served));
        match (target_ref, served_ref) {
            (Some(target_name), Some(served_name)) if target_name != served_name => {
                self.differ(
                    at,
                    format!("target refers to `{target_name}`, served to `{served_name}`"),
                );
                return;
            }
            (Some(name), None) => {
                self.differ(at, format!("target refers to `{name}`, served inlines it"));
                return;
            }
            (None, Some(name)) => {
                self.differ(at, format!("served refers to `{name}`, target inlines it"));
                return;
            }
            _ => {}
        }
        let target = match target_ref {
            Some(name) => match self.target.schema(name) {
                Some(schema) => schema,
                // Dangling in the target: compared by name only.
                None => return,
            },
            None => target,
        };
        let served = match served_ref {
            Some(name) => match self.served.schema(name) {
                Some(schema) => schema,
                None => {
                    self.differ(
                        at,
                        format!("served refers to `{name}`, which it doesn't define"),
                    );
                    return;
                }
            },
            None => served,
        };
        match (target, served) {
            (Value::Object(target), Value::Object(served)) => {
                let keys: BTreeSet<&String> = target
                    .keys()
                    .chain(served.keys())
                    .filter(|key| !ANNOTATIONS.contains(&key.as_str()))
                    .collect();
                for key in keys {
                    let at = join(at, key);
                    match (target.get(key), served.get(key)) {
                        (Some(target), Some(served)) => self.keyword(&at, key, target, served),
                        (Some(value), None) => {
                            self.differ(&at, format!("target has {value}, served has none"));
                        }
                        (None, Some(value)) => {
                            self.differ(&at, format!("served has {value}, target has none"));
                        }
                        (None, None) => {}
                    }
                }
            }
            (target, served) if target != served => {
                self.differ(at, format!("target {target}, served {served}"));
            }
            _ => {}
        }
    }

    /// Compares one keyword's values.
    fn keyword(&mut self, at: &str, key: &str, target: &Value, served: &Value) {
        match key {
            "properties" => {
                let (Some(target), Some(served)) = (target.as_object(), served.as_object()) else {
                    self.differ(at, format!("target {target}, served {served}"));
                    return;
                };
                for (name, schema) in target {
                    match served.get(name) {
                        Some(other) => self.schema(&join(at, name), schema, other),
                        None => self.differ(at, format!("target has `{name}`, served has not")),
                    }
                }
                for name in served.keys().filter(|name| !target.contains_key(*name)) {
                    self.differ(at, format!("served has `{name}`, target has not"));
                }
            }
            "items" | "additionalProperties" | "not" => self.schema(at, target, served),
            "oneOf" | "anyOf" | "allOf" | "prefixItems" => {
                let (Some(target), Some(served)) = (target.as_array(), served.as_array()) else {
                    self.differ(at, format!("target {target}, served {served}"));
                    return;
                };
                if target.len() != served.len() {
                    self.differ(
                        at,
                        format!(
                            "target has {} schemas, served {}",
                            target.len(),
                            served.len()
                        ),
                    );
                    return;
                }
                for (index, (target, served)) in target.iter().zip(served).enumerate() {
                    self.schema(&join(at, &index.to_string()), target, served);
                }
            }
            // Order doesn't matter in these.
            "type" | "required" | "enum" => {
                if as_set(target) != as_set(served) {
                    self.differ(at, format!("target {target}, served {served}"));
                }
            }
            _ => {
                if target != served {
                    self.differ(at, format!("target {target}, served {served}"));
                }
            }
        }
    }
}

/// `a.b`, or `b` when `a` is empty.
fn join(at: &str, key: &str) -> String {
    if at.is_empty() || at.ends_with(": ") {
        format!("{at}{key}")
    } else {
        format!("{at}.{key}")
    }
}

/// A value as a set of its items (a single value is a set of one).
fn as_set(value: &Value) -> BTreeSet<String> {
    match value {
        Value::Array(items) => items.iter().map(Value::to_string).collect(),
        other => BTreeSet::from([other.to_string()]),
    }
}

/// The parameters of an operation, by name and location.
fn parameters(operation: &Value) -> BTreeMap<(String, String), &Value> {
    operation
        .get("parameters")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|parameter| {
            let field = |key| {
                parameter
                    .get(key)
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned()
            };
            ((field("name"), field("in")), parameter)
        })
        .collect()
}

/// The JSON schema of a request body or response, if it has one.
fn json_schema(body: &Value) -> Option<&Value> {
    body.get("content")
        .and_then(|content| content.get("application/json"))
        .and_then(|json| json.get("schema"))
}

/// The lowest `2xx` status of an operation, and its response.
fn success(operation: &Value) -> Option<(&String, &Value)> {
    operation
        .get("responses")
        .and_then(Value::as_object)?
        .iter()
        .filter(|(status, _)| status.starts_with('2'))
        .min_by_key(|(status, _)| status.as_str())
}

/// Every difference between the target's `covered` operations and the served
/// document's, each a line naming the operation and where.
fn differences(target: &Value, served: &Value, covered: &[(&str, &str)]) -> Vec<String> {
    let mut compare = Compare {
        target: Doc {
            root: target,
            name: "target",
        },
        served: Doc {
            root: served,
            name: "served",
        },
        differences: Vec::new(),
    };
    for (method, path) in covered {
        let operation = |doc: &Value| doc.get("paths")?.get(*path)?.get(*method).cloned();
        let at = format!("{} {path}", method.to_uppercase());
        let Some(wanted) = operation(target) else {
            compare.differ(
                &at,
                format!("not an operation of the {}", compare.target.name),
            );
            continue;
        };
        let Some(got) = operation(served) else {
            compare.differ(
                &at,
                format!("not {} by the application", compare.served.name),
            );
            continue;
        };
        for key in ["operationId", "tags"] {
            if wanted.get(key) != got.get(key) {
                compare.differ(
                    &at,
                    format!(
                        "{key}: target {}, served {}",
                        wanted.get(key).unwrap_or(&Value::Null),
                        got.get(key).unwrap_or(&Value::Null)
                    ),
                );
            }
        }

        let (wanted_params, got_params) = (parameters(&wanted), parameters(&got));
        for ((name, place), parameter) in &wanted_params {
            let here = format!("{at}: parameter `{name}` ({place})");
            let Some(other) = got_params.get(&(name.clone(), place.clone())) else {
                compare.differ(&here, "target has it, served has not".to_owned());
                continue;
            };
            let required = |p: &Value| p.get("required").and_then(Value::as_bool).unwrap_or(false);
            if required(parameter) != required(other) {
                compare.differ(
                    &here,
                    format!(
                        "required: target {}, served {}",
                        required(parameter),
                        required(other)
                    ),
                );
            }
            let schema = |p: &Value| p.get("schema").cloned().unwrap_or(Value::Null);
            compare.schema(&format!("{here}: "), &schema(parameter), &schema(other));
        }
        for (name, place) in got_params
            .keys()
            .filter(|key| !wanted_params.contains_key(*key))
        {
            compare.differ(
                &format!("{at}: parameter `{name}` ({place})"),
                "served has it, target has not".to_owned(),
            );
        }

        match (wanted.get("requestBody"), got.get("requestBody")) {
            (Some(wanted_body), Some(got_body)) => {
                let here = format!("{at}: request body");
                if wanted_body.get("required") != got_body.get("required") {
                    compare.differ(&here, "`required` differs".to_owned());
                }
                match (json_schema(wanted_body), json_schema(got_body)) {
                    (Some(wanted), Some(got)) => compare.schema(&format!("{here}: "), wanted, got),
                    (None, None) => {}
                    _ => compare.differ(&here, "only one takes JSON".to_owned()),
                }
            }
            (None, None) => {}
            (Some(_), None) => compare.differ(&at, "target takes a body, served not".to_owned()),
            (None, Some(_)) => compare.differ(&at, "served takes a body, target not".to_owned()),
        }

        match (success(&wanted), success(&got)) {
            (Some((wanted_status, wanted_response)), Some((got_status, got_response))) => {
                if wanted_status != got_status {
                    compare.differ(
                        &at,
                        format!("success status: target {wanted_status}, served {got_status}"),
                    );
                }
                let here = format!("{at}: response {wanted_status}");
                // The allowance (see the module docs): a target response
                // without a body accepts any served body.
                if let Some(wanted) = json_schema(wanted_response) {
                    match json_schema(got_response) {
                        Some(got) => compare.schema(&format!("{here}: "), wanted, got),
                        None => compare.differ(&here, "target has a body, served has none".into()),
                    }
                }
            }
            _ => compare.differ(&at, "no success response in one of them".to_owned()),
        }
    }
    compare.differences
}

/// The document the application serves at `/api-docs/openapi.json`. The
/// pool never connects: serving the document needs no database.
async fn served_document() -> Result<Value, Box<dyn std::error::Error>> {
    let pool = sqlx::PgPool::connect_lazy("postgres://localhost/unused")?;
    let keys = web::Data::new(JwtKeys::new("unused"));
    let app = test::init_service(App::new().configure(|cfg| configure(cfg, &pool, &keys))).await;
    let request = test::TestRequest::get().uri("/api-docs/openapi.json");
    let response = test::call_service(&app, request.to_request()).await;
    assert!(response.status().is_success(), "{}", response.status());
    Ok(serde_json::from_slice(&test::read_body(response).await)?)
}

/// The differences found that [`KNOWN`] doesn't list, and the known ones not
/// found.
fn unexpected(found: &[String]) -> (Vec<&String>, Vec<&str>) {
    let new = found
        .iter()
        .filter(|found| !KNOWN.iter().any(|(known, _)| known == found))
        .collect();
    let gone = KNOWN
        .iter()
        .map(|(known, _)| *known)
        .filter(|known| !found.iter().any(|found| found == known))
        .collect();
    (new, gone)
}

#[actix_web::test]
async fn the_served_document_meets_the_target_for_every_covered_operation()
-> Result<(), Box<dyn std::error::Error>> {
    let target: Value = serde_json::from_str(TARGET)?;
    let served = served_document().await?;
    let found = differences(&target, &served, COVERED);
    let (new, gone) = unexpected(&found);
    assert!(
        new.is_empty() && gone.is_empty(),
        "the served document breaks the contract:\n{}{}",
        new.iter()
            .map(|line| format!("  {line}\n"))
            .collect::<String>(),
        gone.iter()
            .map(|line| format!("  known, but no longer found (remove it from KNOWN): {line}\n"))
            .collect::<String>()
    );
    Ok(())
}

/// A served document that changed a field's type, renamed an operation and
/// lost a parameter breaks the contract, each named in the message.
#[actix_web::test]
async fn a_mismatch_breaks_the_contract_with_a_readable_list()
-> Result<(), Box<dyn std::error::Error>> {
    let target: Value = serde_json::from_str(TARGET)?;
    let mut served = served_document().await?;
    served["components"]["schemas"]["UserResponse"]["properties"]["email"]["type"] =
        Value::from("integer");
    served["paths"]["/api/v1/audit-log"]["get"]["operationId"] = Value::from("entries");
    if let Some(parameters) = served["paths"]["/api/v1/users"]["get"]["parameters"].as_array_mut() {
        parameters.retain(|parameter| parameter["name"] != "q");
    }
    let found = differences(&target, &served, COVERED);
    let (new, _) = unexpected(&found);
    let new: Vec<&str> = new.iter().map(|line| line.as_str()).collect();
    assert_eq!(
        new,
        [
            "GET /api/v1/users: parameter `q` (query): target has it, served has not",
            "GET /api/v1/users/{id}: response 200: properties.email.type: target \"string\", served \"integer\"",
            "PATCH /api/v1/users/{id}: response 200: properties.email.type: target \"string\", served \"integer\"",
            "GET /api/v1/audit-log: operationId: target \"list\", served \"entries\"",
        ]
    );
    Ok(())
}

/// A document holding one operation, `GET /things`, whose `200` response
/// has `schema` and whose components define `schemas`.
fn one_operation(schema: Value, schemas: Value) -> Value {
    serde_json::json!({
        "paths": {"/things": {"get": {"responses": {"200": {
            "content": {"application/json": {"schema": schema}}
        }}}}},
        "components": {"schemas": schemas},
    })
}

/// The differences between two one-operation documents.
fn things_differences(target: &Value, served: &Value) -> Vec<String> {
    differences(target, served, &[("get", "/things")])
}

/// A `$ref` on one side needs a `$ref` to the same name on the other: a
/// schema inlined where the other document names one differs, even when
/// its keywords are the same.
#[actix_web::test]
async fn a_ref_on_either_side_needs_a_ref_of_the_same_name() {
    let thing = serde_json::json!({"type": "object", "properties": {"id": {"type": "string"}}});
    let named = one_operation(
        serde_json::json!({"$ref": "#/components/schemas/Thing"}),
        serde_json::json!({"Thing": thing.clone()}),
    );
    let inlined = one_operation(thing, serde_json::json!({}));
    assert_eq!(
        things_differences(&named, &inlined),
        ["GET /things: response 200: target refers to `Thing`, served inlines it"]
    );
    assert_eq!(
        things_differences(&inlined, &named),
        ["GET /things: response 200: served refers to `Thing`, target inlines it"]
    );
    assert!(things_differences(&named, &named).is_empty());
}

/// A target response without a body documents none, so the application may
/// serve any (the contract's one allowance). A target body the application
/// doesn't serve still breaks it.
#[actix_web::test]
async fn a_target_response_without_a_body_accepts_any_served_body() {
    let bodiless = serde_json::json!({
        "paths": {"/things": {"get": {"responses": {"200": {"description": "OK"}}}}},
    });
    let with_body = one_operation(serde_json::json!({"type": "string"}), serde_json::json!({}));
    assert!(things_differences(&bodiless, &with_body).is_empty());
    assert_eq!(
        things_differences(&with_body, &bodiless),
        ["GET /things: response 200: target has a body, served has none"]
    );
}
