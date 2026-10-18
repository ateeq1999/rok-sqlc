//! The shared tools as the OpenAPI document describes them (`[api]
//! openapi`): [`describe`] adds utoipa's derives to the shared tools'
//! templates, which are written without them, so turning the document off
//! leaves them exactly as they are.

use crate::generator::openapi::{IN_QUERY, INTO_PARAMS, TO_SCHEMA};
use crate::generator::push_derives;

/// `template`, the shared tool `name`, with its exposed types deriving
/// `ToSchema` or `IntoParams`, and its handlers (health's) carrying
/// `#[utoipa::path]`.
pub fn describe(name: &str, template: &str) -> String {
    match name {
        "health.rs" => template
            .replacen(
                "pub async fn live()",
                &format!("{LIVE}pub async fn live()"),
                1,
            )
            .replacen(
                "pub async fn ready(",
                &format!("{READY}pub async fn ready("),
                1,
            ),
        "pagination.rs" => describe_pagination(template),
        "response.rs" => derive(template, "pub struct PaginatedResponse<T> {", TO_SCHEMA),
        "validation.rs" => derive(template, "pub struct IdList<T> {", TO_SCHEMA),
        _ => template.to_owned(),
    }
}

/// `template` with `derive` added to the one-line `#[derive(...)]` right
/// above `head` (or above its `#[serde(...)]` line), laid out as rustfmt
/// lays it out.
fn derive(template: &str, head: &str, derive: &str) -> String {
    let at = template.find(head).unwrap_or(template.len());
    let line = template[..at]
        .rfind("#[derive(")
        .expect("an exposed type has a derive line");
    let end = line
        + template[line..]
            .find('\n')
            .expect("a derive line ends its line")
        + 1;
    let mut derives = template[line..end].to_owned();
    push_derives(&mut derives, &[derive]);
    format!("{}{derives}{}", &template[..line], &template[end..])
}

/// `pagination.rs`: `SortOrder` derives `ToSchema`, and `ListQuery` derives
/// `IntoParams`, its fields in the query. utoipa 5 can't derive
/// `IntoParams` over a generic field, so `sort` refers to `S`'s schema
/// through [`SORT_SCHEMA`], and `S` must be a schema.
fn describe_pagination(template: &str) -> String {
    let text = derive(template, "pub enum SortOrder {", TO_SCHEMA);
    let text = derive(&text, "pub struct ListQuery<S = String> {", INTO_PARAMS);
    text.replacen(
        "pub struct ListQuery<S = String> {",
        &format!("{}pub struct ListQuery<S: utoipa::ToSchema = String> {{", IN_QUERY),
        1,
    )
    .replacen(
        "    /// What to sort by.\n    pub sort: Option<S>,",
        "    /// What to sort by.\n    #[param(schema_with = sort_schema::<S>)]\n    pub sort: Option<S>,",
        1,
    )
    .replacen(
        "impl<S> Default for ListQuery<S> {",
        "impl<S: utoipa::ToSchema> Default for ListQuery<S> {",
        1,
    )
    .replacen(
        "impl<S> ListQuery<S> {",
        "impl<S: utoipa::ToSchema> ListQuery<S> {",
        1,
    )
    .replacen(
        "/// A page of a list: its number",
        &format!("{SORT_SCHEMA}/// A page of a list: its number"),
        1,
    )
}

/// The path of `GET /health/live`.
const LIVE: &str = r#"#[utoipa::path(
    get,
    path = "/health/live",
    tag = "health",
    operation_id = "live",
    responses((status = 200, description = "The server answers.")),
)]
"#;

/// The path of `GET /health/ready`.
const READY: &str = r#"#[utoipa::path(
    get,
    path = "/health/ready",
    tag = "health",
    operation_id = "ready",
    responses(
        (status = 200, description = "The database answers."),
        (status = 503, description = "The database is not reachable.", body = ErrorBody),
    ),
)]
"#;

/// The schema of a list query's `sort`.
const SORT_SCHEMA: &str = "/// The schema of a list query's `sort`: a reference to the sort key's
/// schema, or text for the default, `String`.
fn sort_schema<S: utoipa::ToSchema>() -> utoipa::openapi::RefOr<utoipa::openapi::Schema> {
    if S::name() == <String as utoipa::ToSchema>::name() {
        S::schema()
    } else {
        utoipa::openapi::Ref::from_schema_name(S::name()).into()
    }
}

";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_derive_joins_the_line_above_the_item() {
        let template =
            "/// Doc.\n#[derive(Debug, Clone)]\n#[serde(default)]\npub struct IdList<T> {\n}\n";
        assert_eq!(
            describe("validation.rs", template),
            "/// Doc.\n#[derive(Debug, Clone, utoipa::ToSchema)]\n#[serde(default)]\npub struct IdList<T> {\n}\n"
        );
        assert_eq!(describe("tenant.rs", template), template);
    }
}
