//! `GeneratedApiDoc` in `generated.rs` (ADR 0016): the OpenAPI document of
//! every generated path, a tag per feature, the schemas utoipa can't
//! collect from the paths, and the `BearerAuth` addon defining
//! `bearer_auth`. The scaffolded `src/openapi.rs` merges it into the user's
//! `ApiDoc`.

use super::feature::{Endpoint, Feature};
use super::paths::handler_names;
use super::schemas::{HEALTH_ROUTES, HEALTH_TAG};

/// The imports `generated.rs` needs for the addon.
pub const USES: &str = "use utoipa::openapi::Components;
use utoipa::openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme};
";

/// `GeneratedApiDoc` and its `BearerAuth` addon, over `features`.
pub fn render(features: &[Feature<'_>]) -> String {
    let mut paths: Vec<String> = HEALTH_ROUTES
        .iter()
        .map(|route| format!("crate::shared::health::{}", route.handler))
        .collect();
    // The sort keys and `?include=` names are `$ref`s from the params,
    // which utoipa doesn't follow, and an embedding's response is only
    // described in prose.
    let mut schemas = vec!["crate::shared::pagination::SortOrder".to_owned()];
    let mut tags = vec![HEALTH_TAG.to_owned()];
    for feature in features {
        let module = feature.module();
        let names = handler_names(feature);
        if names.is_empty() {
            continue;
        }
        paths.extend(
            names
                .iter()
                .map(|name| format!("{module}::handlers::{name}")),
        );
        if feature.serves(Endpoint::List) || feature.serves(Endpoint::Search) {
            schemas.push(format!("{module}::dto::{}", feature.sort_enum()));
        }
        let includes = feature.includes();
        if !includes.is_empty() {
            schemas.push(format!("{module}::dto::{}", feature.include_enum()));
            schemas.extend(
                includes
                    .iter()
                    .map(|relation| format!("{module}::dto::{}", relation.with_response(feature))),
            );
        }
        if !tags.contains(&feature.tag) {
            tags.push(feature.tag.clone());
        }
    }
    let list = |items: &[String], indent: &str| -> String {
        items
            .iter()
            .map(|item| format!("{indent}{item},\n"))
            .collect()
    };
    let tags: Vec<String> = tags
        .iter()
        .map(|tag| format!("(name = \"{tag}\")"))
        .collect();
    format!(
        "
/// The `OpenAPI` document of every generated path and schema, with the
/// `bearer_auth` security scheme. `crate::openapi::document` merges it into
/// the hand-written `ApiDoc`.
#[derive(utoipa::OpenApi)]
#[openapi(
    paths(
{}    ),
    components(schemas(
{}    )),
    modifiers(&BearerAuth),
    tags(
{}    ),
)]
pub struct GeneratedApiDoc;

/// Adds the `bearer_auth` security scheme: a JWT as an HTTP bearer token.
pub struct BearerAuth;

impl utoipa::Modify for BearerAuth {{
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {{
        let description = \"A JWT the application signed, sent as `Bearer <token>`.\";
        let scheme = HttpBuilder::new()
            .scheme(HttpAuthScheme::Bearer)
            .bearer_format(\"JWT\")
            .description(Some(description))
            .build();
        openapi
            .components
            .get_or_insert_with(Components::new)
            .add_security_scheme(\"bearer_auth\", SecurityScheme::Http(scheme));
    }}
}}
",
        list(&paths, "        "),
        list(&schemas, "        "),
        list(&tags, "        "),
    )
}
