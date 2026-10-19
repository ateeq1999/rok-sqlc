//! One documented operation of a feature (ADR 0016) and its rendering as
//! `#[utoipa::path(...)]`: [`super::paths`] decides what each handler
//! answers, this module how that reads.

use heck::ToSnakeCase;

use super::feature::Feature;
use super::handlers::key_columns;
use super::layout::Uses;
use super::scope::Caller;
use crate::ApiAccess;
use crate::generator::MAX_WIDTH;
use crate::generator::openapi::value_type;
use crate::schema::{Column, Table};

/// A Rust name as lowercase words: `AuditLogEntry` and `warehouse_code`
/// are "audit log entry" and "warehouse code".
fn words(name: &str) -> String {
    name.to_snake_case().replace('_', " ")
}

/// A row's singular name as words, the first capitalized, as
/// api-starter-axum's OpenAPI document names rows: `users` (`User`) is
/// "User", `audit_log_entry` is "Audit log entry".
fn singular(table: &Table) -> String {
    let words = words(&table.struct_name);
    let mut chars = words.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

/// The description of the path parameter holding `table`'s `column` (a key
/// column, or a unique lookup's): "User id" for `users.id`, "User email"
/// for `users.email`.
pub fn column_description(table: &Table, column: &Column) -> String {
    format!("{} {}", singular(table), words(&column.rust_name))
}

/// Whether an operation needs `bearer_auth`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Security {
    /// A public route.
    None,
    /// A route needing a caller.
    Required,
    /// A public route of which some answers need a caller (`?include=` of a
    /// relationship to a stricter table).
    Optional,
}

/// A response header: its name and description.
pub type Header = (&'static str, &'static str);

/// The `ETag` header of a versioned row's answer.
const ETAG: Header = ("ETag", "The row's version, which `If-Match` sends back.");

/// One documented operation.
pub struct Operation {
    method: &'static str,
    /// The absolute path, `{param}`s included.
    path: String,
    tag: String,
    operation_id: String,
    pub params: Vec<String>,
    request_body: Option<String>,
    /// The success status, its description, body type and headers.
    status: u16,
    pub description: &'static str,
    body: Option<String>,
    pub headers: Vec<Header>,
    /// The error statuses, in any order and possibly repeated.
    pub errors: Vec<u16>,
    pub security: Security,
}

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
pub fn access_errors(access: ApiAccess) -> &'static [u16] {
    match access {
        ApiAccess::Public => &[],
        ApiAccess::User => &[401],
        ApiAccess::Admin => &[401, 403],
    }
}

/// The type a path parameter holding `column` is documented as: a typed id
/// as its inner type, a type utoipa can't describe as text.
fn param_type(column: &Column) -> String {
    let value = column.value_type();
    value_type(column, value).unwrap_or_else(|| value.to_owned())
}

/// Whether a path parameter of `column` can fail to parse (a `400`): any
/// but text.
fn can_fail_to_parse(column: &Column) -> bool {
    param_type(column) != "String"
}

impl Operation {
    /// A `method` operation at `path` under the feature's URL, named
    /// `operation_id`, acting for `caller`.
    pub fn new(
        feature: &Feature<'_>,
        method: &'static str,
        path: &str,
        operation_id: String,
        caller: Caller,
    ) -> Self {
        let security = if caller.access == ApiAccess::Public {
            Security::None
        } else {
            Security::Required
        };
        Operation {
            method,
            path: format!("{}{path}", feature.url),
            tag: feature.tag.clone(),
            operation_id,
            params: Vec::new(),
            request_body: None,
            status: 200,
            description: "",
            body: None,
            headers: Vec::new(),
            errors: access_errors(caller.access).to_vec(),
            security,
        }
    }

    /// Answers `status` with `description` and `body`.
    pub fn answers(mut self, status: u16, description: &'static str, body: Option<String>) -> Self {
        self.status = status;
        self.description = description;
        self.body = body;
        self
    }

    /// The path parameter `name` holding `column`, described as
    /// `description`, and the `400` of a value that can fail to parse.
    pub fn path_param(mut self, name: &str, column: &Column, description: &str) -> Self {
        self.params.push(format!(
            "(\"{name}\" = {}, Path, description = \"{description}\")",
            param_type(column),
        ));
        if can_fail_to_parse(column) {
            self.errors.push(400);
        }
        self
    }

    /// The row's key in the path: `{id}`, or one parameter per key column.
    pub fn keyed(self, feature: &Feature<'_>) -> Self {
        let columns = key_columns(feature);
        if let [column] = columns.as_slice() {
            return self.path_param("id", column, &column_description(feature.table, column));
        }
        columns.into_iter().fold(self, |op, column| {
            let description = column_description(feature.table, column);
            op.path_param(&column.rust_name, column, &description)
        })
    }

    /// The query string `query`: its params, and the `400` of a query that
    /// doesn't parse.
    pub fn query(mut self, query: String) -> Self {
        self.params.push(query);
        self.errors.push(400);
        self
    }

    /// The JSON body `body`: malformed (`400`), over the limit (`413`), or
    /// not of its type or breaking its rules (`422`).
    pub fn json(mut self, body: String) -> Self {
        self.request_body = Some(body);
        self.errors.extend([400, 413, 422]);
        self
    }

    pub fn errors(mut self, errors: &[u16]) -> Self {
        self.errors.extend(errors);
        self
    }

    /// `409` when `conflicts`.
    pub fn conflicts(self, conflicts: bool) -> Self {
        if conflicts { self.errors(&[409]) } else { self }
    }

    /// The `ETag` of a versioned row on the answer.
    pub fn etag(mut self, feature: &Feature<'_>) -> Self {
        if feature.version.is_some() {
            self.headers.push(ETAG);
        }
        self
    }

    /// Renders the operation as `#[utoipa::path(...)]`, one argument per
    /// line.
    pub fn push(&self, output: &mut String, uses: &mut Uses) {
        output.push_str(&format!(
            "#[utoipa::path(\n    {},\n    path = \"{}\",\n    tag = \"{}\",\n    operation_id = \"{}\",\n",
            self.method, self.path, self.tag, self.operation_id
        ));
        push_list(output, "params", &self.params);
        if let Some(body) = &self.request_body {
            output.push_str(&format!("    request_body = {body},\n"));
        }
        let mut responses = vec![self.success()];
        let is_page = |body: &String| body.starts_with("PaginatedResponse<");
        if self.body.as_ref().is_some_and(is_page) {
            uses.add("crate::shared::response::PaginatedResponse");
        }
        let mut errors = self.errors.clone();
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
        match self.security {
            Security::None => {}
            Security::Required => output.push_str("    security((\"bearer_auth\" = [])),\n"),
            Security::Optional => output.push_str("    security((), (\"bearer_auth\" = [])),\n"),
        }
        output.push_str(")]\n");
    }

    /// The success response.
    fn success(&self) -> String {
        let mut fields = vec![
            format!("status = {}", self.status),
            format!("description = \"{}\"", self.description),
        ];
        fields.extend(self.body.as_ref().map(|body| format!("body = {body}")));
        response(&fields, &self.headers)
    }
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
