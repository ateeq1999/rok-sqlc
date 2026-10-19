//! The shared tools as the OpenAPI document describes them (`[api]
//! openapi`): [`describe`] adds utoipa's derives to the shared tools'
//! templates, which are written without them, so turning the document off
//! leaves them exactly as they are.

use crate::generator::openapi::{IN_QUERY, INTO_PARAMS, TO_SCHEMA};
use crate::generator::{push_derives, replace_once};

/// A route of `shared/health.rs`, outside the API prefix: its handler, its
/// path and its `responses(...)`, as api-starter-axum's OpenAPI document
/// has them.
pub struct HealthRoute {
    pub handler: &'static str,
    pub path: &'static str,
    responses: &'static str,
}

/// Health's routes: the one place naming their handlers, which
/// `GeneratedApiDoc` lists and their `#[utoipa::path]`s document.
/// `/health/ready`'s `503` has its body (api-starter-axum's document
/// describes no error bodies).
pub const HEALTH_ROUTES: [HealthRoute; 2] = [
    HealthRoute {
        handler: "liveness",
        path: "/health/live",
        responses: r#"responses((status = 200, description = "The process is running")),"#,
    },
    HealthRoute {
        handler: "readiness",
        path: "/health/ready",
        responses: r#"responses(
        (status = 200, description = "The database is reachable"),
        (status = 503, description = "The database is not reachable", body = ErrorBody),
    ),"#,
    },
];

/// The tag of health's routes.
pub const HEALTH_TAG: &str = "health";

/// `template`, the shared tool `name`, with its exposed types deriving
/// `ToSchema` or `IntoParams`, and its handlers (health's) carrying
/// `#[utoipa::path]`.
pub fn describe(name: &str, template: &str) -> String {
    match name {
        "health.rs" => describe_health(template),
        "pagination.rs" => describe_pagination(template),
        "response.rs" => describe_response(template),
        "validation.rs" => derive(template, "pub struct IdList<T> {", TO_SCHEMA),
        _ => template.to_owned(),
    }
}

/// `response.rs`: `PaginatedResponse` derives `ToSchema`, and its `total`,
/// a count of rows, is at least 0, as api-starter-axum's document
/// has its `u64`.
fn describe_response(template: &str) -> String {
    let text = derive(template, "pub struct PaginatedResponse<T> {", TO_SCHEMA);
    replace_once(
        &text,
        "    pub total: i64,",
        "    #[schema(minimum = 0)]\n    pub total: i64,",
    )
}

/// `health.rs`: each handler of [`HEALTH_ROUTES`] carries its
/// `#[utoipa::path]`.
fn describe_health(template: &str) -> String {
    HEALTH_ROUTES.iter().fold(template.to_owned(), |text, route| {
        let head = format!("pub async fn {}(", route.handler);
        let path = format!(
            "#[utoipa::path(\n    get,\n    path = \"{}\",\n    tag = \"{HEALTH_TAG}\",\n    operation_id = \"{}\",\n    {}\n)]\n",
            route.path, route.handler, route.responses
        );
        replace_once(&text, &head, &format!("{path}{head}"))
    })
}

/// `template` with `derive` added to the one-line `#[derive(...)]` right
/// above `head` (or above its `#[serde(...)]` line), laid out as rustfmt
/// lays it out.
fn derive(template: &str, head: &str, derive: &str) -> String {
    let Some(at) = template.find(head) else {
        debug_assert!(false, "the template no longer holds {head:?}");
        return template.to_owned();
    };
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

/// `pagination.rs`: `SortOrder` derives `ToSchema`, and the two list
/// queries, `ListQuery` and `PageQuery`, derive `IntoParams`, their fields
/// in the query. utoipa 5 can't derive `IntoParams` over a generic field,
/// so `sort` refers to `S`'s schema through [`SORT_SCHEMA`], and `S` must
/// be a schema.
fn describe_pagination(template: &str) -> String {
    let mut text = derive(template, "pub enum SortOrder {", TO_SCHEMA);
    for query in ["ListQuery", "PageQuery"] {
        let head = format!("pub struct {query}<S = String> {{");
        text = derive(&text, &head, INTO_PARAMS);
        text = replace_once(
            &text,
            &head,
            &format!("{IN_QUERY}pub struct {query}<S: utoipa::ToSchema = String> {{"),
        );
        // The first `sort` not yet described: `ListQuery`'s comes first.
        text = replace_once(
            &text,
            "    /// What to sort by.\n    pub sort: Option<S>,",
            "    /// What to sort by.\n    #[param(schema_with = sort_schema::<S>)]\n    pub sort: Option<S>,",
        );
        for item in [
            format!("impl<S> Default for {query}<S> {{"),
            format!("impl<S> {query}<S> {{"),
        ] {
            let bounded = item.replacen("impl<S>", "impl<S: utoipa::ToSchema>", 1);
            text = replace_once(&text, &item, &bounded);
        }
    }
    replace_once(
        &text,
        "/// A page of a list: its number",
        &format!("{SORT_SCHEMA}/// A page of a list: its number"),
    )
}

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
    use super::super::shared::{HEALTH, PAGINATION, RESPONSE, VALIDATION};
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

    // Each template still holds what its patches look for: a patch that
    // misses panics in a debug build, here.

    #[test]
    fn health_routes_are_documented() {
        let text = describe("health.rs", HEALTH);
        for route in &HEALTH_ROUTES {
            let mount = format!("\"{}\", web::get().to({}))", route.path, route.handler);
            assert!(HEALTH.contains(&mount), "{mount}");
            let documented = format!(
                "    operation_id = \"{}\",\n    {}\n)]\npub async fn {}(",
                route.handler, route.responses, route.handler
            );
            assert!(text.contains(&documented), "{text}");
        }
    }

    #[test]
    fn both_list_queries_are_params_with_a_described_sort() {
        let text = describe("pagination.rs", PAGINATION);
        for query in ["ListQuery", "PageQuery"] {
            for expected in [
                format!(
                    "Deserialize, utoipa::IntoParams)]\n{IN_QUERY}pub struct {query}<S: utoipa::ToSchema = String> {{"
                ),
                format!("impl<S: utoipa::ToSchema> Default for {query}<S> {{"),
                format!("impl<S: utoipa::ToSchema> {query}<S> {{"),
            ] {
                assert!(text.contains(&expected), "{expected}\n{text}");
            }
        }
        let described = "#[param(schema_with = sort_schema::<S>)]";
        assert_eq!(text.matches(described).count(), 2, "{text}");
        assert!(text.contains(
            "utoipa::ToSchema)]\n#[serde(rename_all = \"snake_case\")]\npub enum SortOrder"
        ));
        assert!(text.contains("fn sort_schema<S: utoipa::ToSchema>()"));
    }

    #[test]
    fn a_page_and_an_id_list_are_schemas() {
        let text = describe("response.rs", RESPONSE);
        let page = "utoipa::ToSchema)]\npub struct PaginatedResponse<T> {";
        assert!(text.contains(page), "{text}");
        let text = describe("validation.rs", VALIDATION);
        let id_list = "validator::Validate, utoipa::ToSchema,\n)]\npub struct IdList<T> {";
        assert!(text.contains(id_list), "{text}");
    }
}
