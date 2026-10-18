//! A feature's HTTP edge: `routes.rs`, mapping each endpoint's URL to its
//! handler, and `handlers.rs`, where each handler parses the request, calls
//! one service method, and answers.

use super::feature::{Endpoint, Feature, Key};
use super::layout::{Uses, push_call, push_signature};
use super::paths;
use super::relation_routes;
use super::scope;
use crate::generator::{FN_CALL_WIDTH, HEADER, MAX_WIDTH, push_doc};
use crate::schema::{BaseType, Column};

/// Renders `routes.rs`: one scope under the feature's URL, which answers a
/// malformed path or query with the wire standard's `400`. Fixed paths
/// come before the key's, which would match them too.
pub fn render_routes(feature: &Feature<'_>) -> String {
    let mut output = format!(
        "{HEADER}\n//! The `{}` routes, under `{}`.\n\nuse actix_web::web;\n\nuse super::handlers;\nuse crate::shared::validation::{{path_config, query_config}};\n\n",
        feature.table.display_name(),
        feature.url
    );
    push_doc(
        &mut output,
        0,
        &[&format!(
            "Mounts every `{}` endpoint under `{}`.",
            feature.table.display_name(),
            feature.url
        )],
    );
    output.push_str("pub fn routes(cfg: &mut web::ServiceConfig) {\n    cfg.service(\n");
    output.push_str(&format!(
        "        web::scope({})\n            .app_data(path_config())\n            .app_data(query_config())",
        crate::generator::sql::str_literal(&feature.url)
    ));
    let key_path = feature.key_path();
    let mut routes: Vec<(String, &str, String)> = Vec::new();
    for endpoint in &feature.endpoints {
        let path = endpoint.path(&key_path);
        routes.push((path, endpoint.method(), endpoint.name().to_owned()));
    }
    for lookup in &feature.lookups {
        routes.push((lookup.path(), "get", lookup.function.clone()));
    }
    routes.extend(relation_routes::routes(feature));
    // Fewer `{...}` segments first, so `/search` and `/by-...` come before
    // `/{id}`; the order is otherwise kept.
    routes.sort_by_key(|(path, _, _)| path.matches('{').count());
    for (path, method, handler) in &routes {
        let to = format!("web::{method}().to(handlers::{handler})");
        let one_line = format!("            .route(\"{path}\", {to})");
        let args = format!("\"{path}\", {to}");
        if one_line.len() <= MAX_WIDTH && args.len() <= FN_CALL_WIDTH {
            output.push('\n');
            output.push_str(&one_line);
        } else {
            output.push_str(&format!(
                "\n            .route(\n                \"{path}\",\n                {to},\n            )"
            ));
        }
    }
    output.push_str(",\n    );\n}\n");
    output
}

/// Renders `handlers.rs`.
pub fn render_handlers(feature: &Feature<'_>) -> String {
    let mut uses = Uses::default();
    uses.add("actix_web::HttpResponse");
    uses.add("actix_web::web");
    uses.add(&format!("super::dto::{}", feature.response()));
    uses.add(&format!("super::service::{}", feature.service()));
    uses.add(&format!("super::service_api::{}", feature.service_api()));
    uses.add("crate::error::AppError");
    let mut body = String::new();
    for endpoint in &feature.endpoints {
        body.push('\n');
        render_handler(&mut body, &mut uses, feature, *endpoint);
    }
    for lookup in &feature.lookups {
        body.push('\n');
        render_lookup(&mut body, &mut uses, feature, lookup);
    }
    relation_routes::render_handlers(&mut body, &mut uses, feature);
    let mut output = format!(
        "{HEADER}\n//! The `{}` handlers: each parses the request, calls one\n//! [`{}`] method, and answers.\n\n",
        feature.table.display_name(),
        feature.service_api()
    );
    output.push_str(&uses.render());
    output.push_str(body.trim_start_matches('\n'));
    output
}

pub fn key_columns<'a>(feature: &Feature<'a>) -> Vec<&'a Column> {
    match &feature.key {
        None => Vec::new(),
        Some(Key::Single(column)) => vec![column],
        Some(Key::Composite(columns)) => columns.clone(),
    }
}

/// Whether a key column's text may hold characters a path segment can't:
/// any key but a number or a UUID.
fn needs_encoding(column: &Column) -> bool {
    !matches!(column.base_type(), BaseType::Integer | BaseType::Uuid)
}

/// The handler's own scope: the feature's tenant and caller, passing the
/// caller on when rows are owned.
pub fn own_scope(feature: &Feature<'_>, uses: &mut Uses) -> scope::HandlerScope {
    scope::handler(feature, uses, feature.tenant, feature.caller())
}

fn render_handler(output: &mut String, uses: &mut Uses, feature: &Feature<'_>, endpoint: Endpoint) {
    let name = endpoint.name();
    let path = endpoint.path(&feature.key_path());
    let what = match endpoint {
        Endpoint::List => "one page of rows, `200`.",
        Endpoint::Search => "one page of the rows the body's filter matches, `200`.",
        Endpoint::Create if feature.key.is_some() => "creates a row, `201` with its `Location`.",
        Endpoint::Create => "creates a row, `201`.",
        Endpoint::GetOne => "the row, `200`.",
        Endpoint::Update => "changes the row, `200` with it.",
        Endpoint::Remove if feature.soft_delete => {
            "soft-deletes the row (deletes it with `?permanent=true`), `204`."
        }
        Endpoint::Remove => "deletes the row, `204`.",
        Endpoint::Restore => "restores the soft-deleted row, `200` with it.",
    };
    let mut doc = vec![format!(
        "`{} {}{path}`: {what}",
        endpoint.http_method(),
        feature.url
    )];
    if endpoint == Endpoint::Update && feature.version.is_some() {
        doc.push("`If-Match` must hold the row's current `ETag`.".to_owned());
    }
    let includes = endpoint == Endpoint::GetOne && !feature.includes().is_empty();
    if includes {
        doc.push("`?include=` embeds a to-one relationship.".to_owned());
    }
    let doc: Vec<&str> = doc.iter().map(String::as_str).collect();
    push_doc(output, 0, &doc);
    paths::push_endpoint(output, uses, feature, endpoint);
    if includes {
        relation_routes::render_get_one(output, uses, feature);
        return;
    }

    let scope = own_scope(feature, uses);
    let mut params = vec![format!("service: web::Data<{}>", feature.service())];
    params.extend(scope.params);
    let key = feature.key_type().unwrap_or_default();
    if endpoint.takes_key() {
        uses.add(&format!("super::model::{key}"));
        params.push(format!("path: web::Path<{key}>"));
    }
    match endpoint {
        Endpoint::List => {
            uses.add(&format!("super::dto::{}", feature.list_query()));
            params.push(format!("query: web::Query<{}>", feature.list_query()));
        }
        Endpoint::Search => {
            uses.add("crate::shared::validation::ValidatedJson");
            uses.add(&format!("super::dto::{}", feature.search_body()));
            params.push(format!("body: ValidatedJson<{}>", feature.search_body()));
        }
        Endpoint::Create => {
            uses.add("crate::shared::validation::ValidatedJson");
            uses.add(&format!("super::dto::{}", feature.create_dto()));
            params.push(format!("body: ValidatedJson<{}>", feature.create_dto()));
        }
        Endpoint::Update => {
            if let Some(version) = feature.version {
                uses.add("crate::shared::validation::IfMatch");
                params.push(format!("version: IfMatch<{}>", version.value_type()));
            }
            uses.add("crate::shared::validation::ValidatedJson");
            uses.add(&format!("super::dto::{}", feature.update_dto()));
            params.push(format!("body: ValidatedJson<{}>", feature.update_dto()));
        }
        Endpoint::Remove if feature.soft_delete => {
            uses.add(&format!("super::dto::{}", feature.remove_query()));
            params.push(format!("query: web::Query<{}>", feature.remove_query()));
        }
        Endpoint::GetOne | Endpoint::Remove | Endpoint::Restore => {}
    }
    push_signature(
        output,
        0,
        &format!("pub async fn {name}"),
        &params,
        "Result<HttpResponse, AppError>",
    );
    let mut args = scope.args;
    match endpoint {
        Endpoint::List | Endpoint::Search => {
            let input = if endpoint == Endpoint::List {
                "query.into_inner()"
            } else {
                "body.into_inner()"
            };
            args.push(input.to_owned());
            let callee = format!("service.{name}");
            push_call(output, 4, "let page = ", &callee, &args, ".await?;");
            output.push_str(&format!(
                "    let response = page.map({}::from);\n",
                feature.response()
            ));
            output.push_str("    Ok(HttpResponse::Ok().json(response))\n");
        }
        Endpoint::Remove => {
            output.push_str("    let key = path.into_inner();\n");
            args.push("key".to_owned());
            if feature.soft_delete {
                output.push_str("    if query.permanent {\n");
                let callee = "service.remove_permanently";
                push_call(output, 8, "", callee, &args, ".await?;");
                output.push_str("    } else {\n");
                push_call(output, 8, "", "service.remove", &args, ".await?;");
                output.push_str("    }\n");
            } else {
                push_call(output, 4, "", "service.remove", &args, ".await?;");
            }
            output.push_str("    Ok(HttpResponse::NoContent().finish())\n");
        }
        _ => {
            match endpoint {
                Endpoint::Create => args.push("body.into_inner()".to_owned()),
                Endpoint::Update => {
                    output.push_str("    let key = path.into_inner();\n");
                    args.push("key".to_owned());
                    if feature.version.is_some() {
                        args.push("version.into_inner()".to_owned());
                    }
                    args.push("body.into_inner()".to_owned());
                }
                _ => args.push("path.into_inner()".to_owned()),
            }
            let callee = format!("service.{name}");
            push_call(output, 4, "let row = ", &callee, &args, ".await?;");
            let status = if endpoint == Endpoint::Create {
                "Created"
            } else {
                "Ok"
            };
            push_row_answer(output, uses, feature, status);
        }
    }
    output.push_str("}\n");
}

/// A `GET /by-<column>/{value}` handler.
fn render_lookup(
    output: &mut String,
    uses: &mut Uses,
    feature: &Feature<'_>,
    lookup: &super::feature::Lookup<'_>,
) {
    push_doc(
        output,
        0,
        &[&format!(
            "`GET {}{}`: the row whose `{}` is the value, `200`.",
            feature.url,
            lookup.path(),
            lookup.column.sql_name
        )],
    );
    paths::push_lookup(output, uses, feature, lookup);
    let scope = own_scope(feature, uses);
    let mut params = vec![format!("service: web::Data<{}>", feature.service())];
    params.extend(scope.params);
    scope::import_type(feature, uses, lookup.column);
    params.push(format!("path: web::Path<{}>", lookup.column.value_type()));
    push_signature(
        output,
        0,
        &format!("pub async fn {}", lookup.function),
        &params,
        "Result<HttpResponse, AppError>",
    );
    let mut args = scope.args;
    args.push("path.into_inner()".to_owned());
    let callee = format!("service.{}", lookup.function);
    push_call(output, 4, "let row = ", &callee, &args, ".await?;");
    push_row_answer(output, uses, feature, "Ok");
    output.push_str("}\n");
}

/// Answers `row` with `status` (`Ok` or `Created`): its `Location` when
/// created, its `ETag` on a versioned table, and the response body.
pub fn push_row_answer(output: &mut String, uses: &mut Uses, feature: &Feature<'_>, status: &str) {
    let mut headers = Vec::new();
    if status == "Created" && feature.key.is_some() {
        push_location(output, uses, feature);
        headers.push("(header::LOCATION, location)");
    }
    if let Some(version) = feature.version {
        uses.add("crate::shared::response::etag");
        output.push_str(&format!(
            "    let etag = etag(&row.{});\n",
            version.rust_name
        ));
        headers.push("(header::ETAG, etag)");
    }
    output.push_str(&format!(
        "    let response = {}::from(row);\n",
        feature.response()
    ));
    push_answer(output, uses, status, &headers, "response");
}

/// `Ok(HttpResponse::<status>()...json(<body>))`, one header per line.
pub fn push_answer(
    output: &mut String,
    uses: &mut Uses,
    status: &str,
    headers: &[&str],
    body: &str,
) {
    if headers.is_empty() {
        output.push_str(&format!("    Ok(HttpResponse::{status}().json({body}))\n"));
        return;
    }
    uses.add("actix_web::http::header");
    output.push_str(&format!("    Ok(HttpResponse::{status}()\n"));
    for header in headers {
        output.push_str(&format!("        .insert_header({header})\n"));
    }
    output.push_str(&format!("        .json({body}))\n"));
}

/// `let location = format!("<url>/{}", ...);`: the created row's URL, each
/// key segment percent-encoded when it may need it.
fn push_location(output: &mut String, uses: &mut Uses, feature: &Feature<'_>) {
    let columns = key_columns(feature);
    let template: String = columns.iter().map(|_| "/{}").collect();
    let args: Vec<String> = columns
        .iter()
        .map(|column| {
            if needs_encoding(column) {
                uses.add("crate::shared::response::path_segment");
                format!("path_segment(&row.{})", column.rust_name)
            } else {
                format!("row.{}", column.rust_name)
            }
        })
        .collect();
    let mut all = vec![format!("\"{}{template}\"", feature.url)];
    all.extend(args);
    let joined = all.join(", ");
    let one_line = format!("    let location = format!({joined});");
    if joined.len() <= FN_CALL_WIDTH && one_line.len() <= MAX_WIDTH {
        output.push_str(&one_line);
        output.push('\n');
        return;
    }
    // rustfmt adds no trailing comma to a macro's arguments.
    output.push_str(&format!(
        "    let location = format!(\n        {}\n    );\n",
        all.join(",\n        ")
    ));
}
