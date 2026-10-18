//! The OpenAPI side of a feature's handlers (ADR 0016): the
//! `#[utoipa::path]` above each one, documenting its method, absolute path,
//! tag, operation id (the handler's name), params, request body, `bearer_auth`
//! when it needs a caller, and exactly the statuses it can answer, each
//! error with the wire standard's `ErrorBody`.

use super::feature::{Endpoint, Feature, Lookup};
use super::handlers::key_columns;
use super::layout::Uses;
use super::relations::{Kind, Relation};
use super::scope::Caller;
use crate::ApiAccess;
use crate::generator::MAX_WIDTH;
use crate::generator::openapi::value_type;
use crate::schema::{Column, RelationshipKind};

/// Whether an operation needs `bearer_auth`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Security {
    /// A public route.
    None,
    /// A route needing a caller.
    Required,
    /// A public route of which some answers need a caller (`?include=` of a
    /// relationship to a stricter table).
    Optional,
}

/// A response header: its name and description.
type Header = (&'static str, &'static str);

/// One documented operation.
struct Operation {
    method: &'static str,
    /// The absolute path, `{param}`s included.
    path: String,
    tag: String,
    operation_id: String,
    params: Vec<String>,
    request_body: Option<String>,
    /// The success status, its description, body type and headers.
    status: u16,
    description: &'static str,
    body: Option<String>,
    headers: Vec<Header>,
    /// The error statuses, in any order and possibly repeated.
    errors: Vec<u16>,
    security: Security,
}

/// The `ETag` header of a versioned row's answer.
const ETAG: Header = ("ETag", "The row's version, which `If-Match` sends back.");

/// The `Location` header of a created row.
const LOCATION: Header = ("Location", "The created row's URL.");

/// The description of an error status, short enough for its response to
/// fit a line.
fn error_description(status: u16) -> &'static str {
    match status {
        400 => "A malformed path, query or body.",
        401 => "No valid bearer token.",
        403 => "The caller is not an admin.",
        404 => "No such row, or not the caller's.",
        409 => "A unique or foreign-key violation.",
        412 => "`If-Match` holds a stale version.",
        413 => "The body is over the limit.",
        422 => "The body breaks a rule or a check.",
        428 => "`If-Match` is missing.",
        _ => "Something went wrong.",
    }
}

/// The statuses a caller's extractor answers before the handler runs.
fn access_errors(access: ApiAccess) -> &'static [u16] {
    match access {
        ApiAccess::Public => &[],
        ApiAccess::User => &[401],
        ApiAccess::Admin => &[401, 403],
    }
}

/// The security of a route acting for `caller`.
fn security(caller: Caller) -> Security {
    if caller.access == ApiAccess::Public {
        Security::None
    } else {
        Security::Required
    }
}

/// The type a path parameter holding `column` is documented as: a typed id
/// as its inner type, a type utoipa can't describe as text.
fn param_type(column: &Column) -> String {
    let value = column.value_type();
    value_type(column, value).unwrap_or_else(|| value.to_owned())
}

/// The path parameter `name` holding `column`.
fn path_param(name: &str, column: &Column) -> String {
    format!(
        "(\"{name}\" = {}, Path, description = \"The `{}` column.\")",
        param_type(column),
        column.sql_name
    )
}

/// The key's path parameters: `{id}`, or one per key column.
fn key_params(feature: &Feature<'_>) -> Vec<String> {
    let columns = key_columns(feature);
    if let [column] = columns.as_slice() {
        return vec![path_param("id", column)];
    }
    columns
        .iter()
        .map(|column| path_param(&column.rust_name, column))
        .collect()
}

/// Whether a path parameter of `column` can fail to parse (a `400`): any
/// but text.
fn parses(column: &Column) -> bool {
    param_type(column) != "String"
}

/// Whether the key's path can fail to parse.
fn key_parses(feature: &Feature<'_>) -> bool {
    key_columns(feature).into_iter().any(parses)
}

/// Whether writing `feature`'s row can break a unique key or a foreign key.
fn insert_conflicts(feature: &Feature<'_>) -> bool {
    let table = feature.table;
    !table.primary_key.is_empty() || !table.unique_keys.is_empty() || update_conflicts(feature)
}

/// Whether changing `feature`'s row can break a unique or foreign key.
fn update_conflicts(feature: &Feature<'_>) -> bool {
    !feature.table.unique_keys.is_empty() || !feature.table.foreign_keys.is_empty()
}

/// Whether deleting `feature`'s row can break another table's foreign key.
fn delete_conflicts(feature: &Feature<'_>) -> bool {
    feature
        .table
        .relationships
        .iter()
        .any(|relationship| relationship.kind != RelationshipKind::BelongsTo)
}

impl Operation {
    /// A `method` operation at `path` under the feature's URL, named
    /// `operation_id`, acting for `caller`.
    fn new(
        feature: &Feature<'_>,
        method: &'static str,
        path: &str,
        operation_id: &str,
        caller: Caller,
    ) -> Self {
        Operation {
            method,
            path: format!("{}{path}", feature.url),
            tag: feature.tag.clone(),
            operation_id: operation_id.to_owned(),
            params: Vec::new(),
            request_body: None,
            status: 200,
            description: "",
            body: None,
            headers: Vec::new(),
            errors: access_errors(caller.access).to_vec(),
            security: security(caller),
        }
    }

    /// Answers `status` with `description` and `body`.
    fn answers(mut self, status: u16, description: &'static str, body: Option<String>) -> Self {
        self.status = status;
        self.description = description;
        self.body = body;
        self
    }

    /// The row's key in the path: its params, and the `400` of a key that
    /// doesn't parse.
    fn keyed(mut self, feature: &Feature<'_>) -> Self {
        self.params.extend(key_params(feature));
        if key_parses(feature) {
            self.errors.push(400);
        }
        self
    }

    /// The query string `query`: its params, and the `400` of a query that
    /// doesn't parse.
    fn query(mut self, query: String) -> Self {
        self.params.push(query);
        self.errors.push(400);
        self
    }

    /// The JSON body `body`: malformed (`400`), over the limit (`413`), or
    /// not of its type or breaking its rules (`422`).
    fn json(mut self, body: String) -> Self {
        self.request_body = Some(body);
        self.errors.extend([400, 413, 422]);
        self
    }

    fn errors(mut self, errors: &[u16]) -> Self {
        self.errors.extend(errors);
        self
    }

    /// The `ETag` of a versioned row on the answer.
    fn etag(mut self, feature: &Feature<'_>) -> Self {
        if feature.version.is_some() {
            self.headers.push(ETAG);
        }
        self
    }
}

/// The operation of `endpoint`.
fn endpoint_operation(feature: &Feature<'_>, endpoint: Endpoint) -> Operation {
    let path = endpoint.path(&feature.key_path());
    let op = Operation::new(
        feature,
        endpoint.method(),
        &path,
        endpoint.name(),
        feature.caller(),
    );
    let response = feature.response();
    let page = format!("PaginatedResponse<{response}>");
    match endpoint {
        Endpoint::List => {
            op.query(feature.list_query())
                .answers(200, "One page of rows.", Some(page))
        }
        Endpoint::Search => op.json(feature.search_body()).answers(
            200,
            "One page of the rows the filter matches.",
            Some(page),
        ),
        Endpoint::Create => {
            let mut op =
                op.json(feature.create_dto())
                    .answers(201, "The row, created.", Some(response));
            if insert_conflicts(feature) {
                op.errors.push(409);
            }
            if feature.key.is_some() {
                op.headers.push(LOCATION);
            }
            op.etag(feature)
        }
        Endpoint::GetOne => {
            let mut op = op
                .keyed(feature)
                .errors(&[404])
                .answers(200, "The row.", Some(response))
                .etag(feature);
            let includes: Vec<&Relation<'_>> = feature
                .relations
                .iter()
                .filter(|relation| matches!(relation.kind, Kind::ToOne { .. }))
                .collect();
            if !includes.is_empty() {
                op = op.query(feature.get_query());
                op.description = "The row, with the relationship `?include=` names embedded.";
                for relation in includes {
                    let caller = relation.caller(feature);
                    op.errors.extend(access_errors(caller.access));
                    if op.security == Security::None && caller.access != ApiAccess::Public {
                        op.security = Security::Optional;
                    }
                }
            }
            op
        }
        Endpoint::Update => {
            let mut op = op.keyed(feature);
            if feature.version.is_some() {
                op.params.push(
                    "(\"If-Match\" = String, Header, description = \"The row's `ETag`.\")"
                        .to_owned(),
                );
                op.errors.extend([412, 428]);
            }
            let mut op = op
                .json(feature.update_dto())
                .errors(&[404])
                .answers(200, "The row, changed.", Some(response))
                .etag(feature);
            if update_conflicts(feature) {
                op.errors.push(409);
            }
            op
        }
        Endpoint::Remove => {
            let mut op = op.keyed(feature).errors(&[404]);
            op = if feature.soft_delete {
                op.query(feature.remove_query()).answers(
                    204,
                    "The row, soft-deleted (deleted with `?permanent=true`).",
                    None,
                )
            } else {
                op.answers(204, "The row, deleted.", None)
            };
            if delete_conflicts(feature) {
                op.errors.push(409);
            }
            op
        }
        Endpoint::Restore => {
            let mut op = op
                .keyed(feature)
                .errors(&[404])
                .answers(200, "The row, restored.", Some(response))
                .etag(feature);
            if !feature.table.unique_keys.is_empty() {
                op.errors.push(409);
            }
            op
        }
    }
}

/// The operation of a `GET /by-<column>/{value}` route.
fn lookup_operation(feature: &Feature<'_>, lookup: &Lookup<'_>) -> Operation {
    let mut op = Operation::new(
        feature,
        "get",
        &lookup.path(),
        &lookup.function,
        feature.caller(),
    )
    .errors(&[404])
    .answers(200, "The row.", Some(feature.response()))
    .etag(feature);
    op.params.push(path_param("value", lookup.column));
    if parses(lookup.column) {
        op.errors.push(400);
    }
    op
}

/// The operations of a to-many relation: its list, and its sync, attach
/// and detach when served, with their handler names.
fn relation_operations(feature: &Feature<'_>, relation: &Relation<'_>) -> Vec<Operation> {
    let caller = relation.caller(feature);
    let segment = format!("{}/{}", feature.key_path(), relation.segment());
    let base = |method, path: &str, name: &str| {
        let id = format!("{name}_{}", relation.name);
        Operation::new(feature, method, path, &id, caller)
            .keyed(feature)
            .errors(&[404])
    };
    let mut operations = vec![base("get", &segment, "list").answers(
        200,
        "The related rows.",
        Some(format!("Vec<{}>", relation.response())),
    )];
    let Some(writes) = relation.writes() else {
        return operations;
    };
    let other = format!("{segment}/{{other}}");
    let paired = |op: Operation| {
        let mut op = op;
        op.params.push(path_param("other", writes.other));
        if parses(writes.other) {
            op.errors.push(400);
        }
        op
    };
    if relation.syncs() {
        operations.push(
            base("put", &segment, "sync")
                .json(format!("IdList<{}>", relation.target_key()))
                .errors(&[409])
                .answers(204, "The row, paired with exactly the rows named.", None),
        );
    }
    if relation.attaches() {
        operations.push(paired(
            base("put", &other, "attach")
                .errors(&[409])
                .answers(204, "The rows, paired.", None),
        ));
    }
    operations.push(paired(base("delete", &other, "detach").answers(
        204,
        "The rows, unpaired.",
        None,
    )));
    operations
}

/// Adds the `#[utoipa::path]` of `endpoint`'s handler, with
/// `feature.openapi`.
pub fn push_endpoint(
    output: &mut String,
    uses: &mut Uses,
    feature: &Feature<'_>,
    endpoint: Endpoint,
) {
    if feature.openapi {
        push(output, uses, &endpoint_operation(feature, endpoint));
    }
}

/// Adds the `#[utoipa::path]` of a lookup's handler, with
/// `feature.openapi`.
pub fn push_lookup(
    output: &mut String,
    uses: &mut Uses,
    feature: &Feature<'_>,
    lookup: &Lookup<'_>,
) {
    if feature.openapi {
        push(output, uses, &lookup_operation(feature, lookup));
    }
}

/// Adds the `#[utoipa::path]` of the relation's handler `name`
/// (`list_labels`), with `feature.openapi`.
pub fn push_relation(
    output: &mut String,
    uses: &mut Uses,
    feature: &Feature<'_>,
    relation: &Relation<'_>,
    name: &str,
) {
    if !feature.openapi {
        return;
    }
    if let Some(op) = relation_operations(feature, relation)
        .into_iter()
        .find(|op| op.operation_id == name)
    {
        push(output, uses, &op);
    }
}

/// The handlers whose paths `GeneratedApiDoc` lists, in the order
/// `handlers.rs` holds them.
pub fn handler_names(feature: &Feature<'_>) -> Vec<String> {
    let mut names: Vec<String> = feature
        .endpoints
        .iter()
        .map(|endpoint| endpoint.name().to_owned())
        .collect();
    names.extend(feature.lookups.iter().map(|lookup| lookup.function.clone()));
    for relation in &feature.relations {
        if let Kind::ToMany { .. } = relation.kind {
            names.extend(
                relation_operations(feature, relation)
                    .into_iter()
                    .map(|op| op.operation_id),
            );
        }
    }
    names
}

/// Renders `op` as `#[utoipa::path(...)]`, one argument per line.
fn push(output: &mut String, uses: &mut Uses, op: &Operation) {
    output.push_str(&format!(
        "#[utoipa::path(\n    {},\n    path = \"{}\",\n    tag = \"{}\",\n    operation_id = \"{}\",\n",
        op.method, op.path, op.tag, op.operation_id
    ));
    push_list(output, "params", &op.params);
    if let Some(body) = &op.request_body {
        output.push_str(&format!("    request_body = {body},\n"));
    }
    let mut responses = vec![success(op)];
    let paged = op.body.as_ref();
    if paged.is_some_and(|body| body.starts_with("PaginatedResponse<")) {
        uses.add("crate::shared::response::PaginatedResponse");
    }
    let mut errors = op.errors.clone();
    errors.sort_unstable();
    errors.dedup();
    if !errors.is_empty() {
        uses.add("crate::shared::response::ErrorBody");
    }
    responses.extend(errors.into_iter().map(|status| {
        let fields = vec![
            format!("status = {status}"),
            format!("description = \"{}\"", error_description(status)),
            "body = ErrorBody".to_owned(),
        ];
        response(&fields, &[])
    }));
    push_list(output, "responses", &responses);
    match op.security {
        Security::None => {}
        Security::Required => output.push_str("    security((\"bearer_auth\" = [])),\n"),
        Security::Optional => output.push_str("    security((), (\"bearer_auth\" = [])),\n"),
    }
    output.push_str(")]\n");
}

/// The success response of `op`.
fn success(op: &Operation) -> String {
    let mut fields = vec![
        format!("status = {}", op.status),
        format!("description = \"{}\"", op.description),
    ];
    fields.extend(op.body.as_ref().map(|body| format!("body = {body}")));
    response(&fields, &op.headers)
}

/// A response of `responses(...)`: on one line when it has no headers and
/// fits, otherwise a field per line and a header per line.
fn response(fields: &[String], headers: &[Header]) -> String {
    let one_line = format!("({})", fields.join(", "));
    if headers.is_empty() && format!("        {one_line},").len() <= MAX_WIDTH {
        return one_line;
    }
    let mut text = String::from("(\n");
    for field in fields {
        text.push_str(&format!("            {field},\n"));
    }
    if !headers.is_empty() {
        text.push_str("            headers(\n");
        for (name, description) in headers {
            text.push_str(&format!(
                "                (\"{name}\" = String, description = \"{description}\"),\n"
            ));
        }
        text.push_str("            ),\n");
    }
    text.push_str("        )");
    text
}

/// `    name(item),` when the one item fits a line, otherwise one item per
/// line; nothing for no items.
fn push_list(output: &mut String, name: &str, items: &[String]) {
    if let [item] = items {
        let line = format!("    {name}({item}),");
        if line.len() <= MAX_WIDTH && !item.contains('\n') {
            output.push_str(&line);
            output.push('\n');
            return;
        }
    }
    if items.is_empty() {
        return;
    }
    output.push_str(&format!("    {name}(\n"));
    for item in items {
        output.push_str(&format!("        {item},\n"));
    }
    output.push_str("    ),\n");
}
