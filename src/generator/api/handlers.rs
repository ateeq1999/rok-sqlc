//! A feature's HTTP edge: `routes.rs`, mapping each endpoint's URL to its
//! handler, and `handlers.rs`, where each handler parses the request, calls
//! one service method, and answers.

use super::feature::{Endpoint, Feature, Key};
use super::layout::{push_signature, push_use};
use crate::generator::{FN_CALL_WIDTH, HEADER, MAX_WIDTH, push_doc};
use crate::schema::{BaseType, Column};

/// Renders `routes.rs`: one scope under the feature's URL, which answers a
/// malformed path or query with the wire standard's `400`.
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
    for endpoint in &feature.endpoints {
        let path = if endpoint.takes_key() {
            feature.key_path()
        } else {
            String::new()
        };
        let to = format!(
            "web::{}().to(handlers::{})",
            endpoint.method(),
            endpoint.name()
        );
        let one_line = format!("            .route(\"{path}\", {to})");
        if one_line.len() <= MAX_WIDTH {
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
    let mut output = format!(
        "{HEADER}\n//! The `{}` handlers: each parses the request, calls one\n//! [`{}`] method, and answers.\n\n",
        feature.table.display_name(),
        feature.service_api()
    );
    output.push_str(&imports(feature));
    for endpoint in &feature.endpoints {
        output.push('\n');
        render_handler(&mut output, feature, *endpoint);
    }
    output
}

fn imports(feature: &Feature<'_>) -> String {
    let location = feature.serves(Endpoint::Create) && feature.key.is_some();
    let encoded = location
        && key_columns(feature)
            .iter()
            .any(|column| needs_encoding(column));
    let mut output = String::new();
    if location {
        output.push_str("use actix_web::http::header;\n");
    }
    output.push_str("use actix_web::{HttpResponse, web};\n\n");
    let mut dto = Vec::new();
    if feature.serves(Endpoint::Create) {
        dto.push(feature.create_dto());
    }
    if feature.serves(Endpoint::List) {
        dto.push(feature.list_query());
    }
    dto.push(feature.response());
    if feature.serves(Endpoint::Update) {
        dto.push(feature.update_dto());
    }
    dto.sort();
    let dto: Vec<&str> = dto.iter().map(String::as_str).collect();
    push_use(&mut output, "super::dto", &dto);
    if feature
        .endpoints
        .iter()
        .any(|endpoint| endpoint.takes_key())
    {
        let key = feature.key_type().unwrap_or_default();
        push_use(&mut output, "super::model", &[&key]);
    }
    output.push_str(&format!("use super::service::{};\n", feature.service()));
    output.push_str(&format!(
        "use super::service_api::{};\n",
        feature.service_api()
    ));
    output.push_str("use crate::error::AppError;\n");
    if encoded {
        output.push_str("use crate::shared::response::path_segment;\n");
    }
    if feature.serves(Endpoint::Create) || feature.serves(Endpoint::Update) {
        output.push_str("use crate::shared::validation::ValidatedJson;\n");
    }
    output
}

fn key_columns<'a>(feature: &Feature<'a>) -> Vec<&'a Column> {
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

fn render_handler(output: &mut String, feature: &Feature<'_>, endpoint: Endpoint) {
    let name = endpoint.name();
    let path = if endpoint.takes_key() {
        feature.key_path()
    } else {
        String::new()
    };
    let what = match endpoint {
        Endpoint::List => "one page of rows, `200`.",
        Endpoint::Create if feature.key.is_some() => "creates a row, `201` with its `Location`.",
        Endpoint::Create => "creates a row, `201`.",
        Endpoint::GetOne => "the row, `200`.",
        Endpoint::Update => "changes the row, `200` with it.",
        Endpoint::Remove => "deletes the row, `204`.",
    };
    push_doc(
        output,
        0,
        &[&format!(
            "`{} {}{path}`: {what}",
            endpoint.http_method(),
            feature.url
        )],
    );
    let mut params = vec![format!("service: web::Data<{}>", feature.service())];
    let key = feature.key_type().unwrap_or_default();
    match endpoint {
        Endpoint::List => params.push(format!("query: web::Query<{}>", feature.list_query())),
        Endpoint::Create => params.push(format!("body: ValidatedJson<{}>", feature.create_dto())),
        Endpoint::GetOne | Endpoint::Remove => params.push(format!("path: web::Path<{key}>")),
        Endpoint::Update => {
            params.push(format!("path: web::Path<{key}>"));
            params.push(format!("body: ValidatedJson<{}>", feature.update_dto()));
        }
    }
    push_signature(
        output,
        0,
        &format!("pub async fn {name}"),
        &params,
        "Result<HttpResponse, AppError>",
    );
    let response = feature.response();
    let (call, from) = match endpoint {
        Endpoint::List => ("let page = service.list(query.into_inner())", "page.map"),
        Endpoint::Create => ("let row = service.create(body.into_inner())", "row"),
        Endpoint::GetOne => ("let row = service.get_one(path.into_inner())", "row"),
        Endpoint::Update => {
            output.push_str("    let key = path.into_inner();\n");
            ("let row = service.update(key, body.into_inner())", "row")
        }
        Endpoint::Remove => ("service.remove(path.into_inner())", ""),
    };
    output.push_str(&format!("    {call}.await?;\n"));
    if endpoint == Endpoint::Create && feature.key.is_some() {
        push_location(output, feature);
    }
    // The response is built first, so each answer is one short chain.
    match from {
        "" => {}
        "row" => output.push_str(&format!("    let response = {response}::from(row);\n")),
        _ => output.push_str(&format!("    let response = page.map({response}::from);\n")),
    }
    match endpoint {
        Endpoint::List | Endpoint::GetOne | Endpoint::Update => {
            output.push_str("    Ok(HttpResponse::Ok().json(response))\n");
        }
        Endpoint::Create if feature.key.is_some() => {
            output.push_str(
                "    Ok(HttpResponse::Created()\n        .insert_header((header::LOCATION, location))\n        .json(response))\n",
            );
        }
        Endpoint::Create => output.push_str("    Ok(HttpResponse::Created().json(response))\n"),
        Endpoint::Remove => output.push_str("    Ok(HttpResponse::NoContent().finish())\n"),
    }
    output.push_str("}\n");
}

/// `let location = format!("<url>/{}", ...);`: the created row's URL, each
/// key segment percent-encoded when it may need it.
fn push_location(output: &mut String, feature: &Feature<'_>) {
    let columns = key_columns(feature);
    let template: String = columns.iter().map(|_| "/{}").collect();
    let args: Vec<String> = columns
        .iter()
        .map(|column| {
            if needs_encoding(column) {
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
