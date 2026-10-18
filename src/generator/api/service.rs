//! A feature's service (ADR 0016): `service_api.rs`, the generated trait
//! with one method per endpoint, each defaulting to the data layer, and
//! `service.rs`, the scaffolded struct implementing it, which the user owns.

use super::feature::{Endpoint, Feature, Key};
use super::layout::{push_signature, push_use};
use crate::generator::{HEADER, push_doc};
use crate::output::SCAFFOLD_HEADER;

/// Renders `service_api.rs`.
pub fn render_api(feature: &Feature<'_>) -> String {
    let trait_name = feature.service_api();
    let mut output = format!(
        "{HEADER}\n//! [`{trait_name}`]: the `{}` feature's operations, one per endpoint,\n//! each with a working default over the data layer.\n\n",
        feature.table.display_name()
    );
    output.push_str(&imports(feature));

    push_doc(
        &mut output,
        0,
        &[
            &format!(
                "The `{}` feature's operations, called by its handlers. Every method but",
                feature.table.display_name()
            ),
            "[`pool`](Self::pool) has a default over the data layer: override one in",
            "`service.rs` to change it. A write comes in two parts: `<write>_in` holds",
            "the logic on a connection the caller owns, so another feature's port can",
            "run it in its own transaction, and `<write>` runs it in a transaction of",
            "its own.",
        ],
    );
    output.push_str(
        "#[allow(\n    async_fn_in_trait,\n    reason = \"services are used as concrete types, never as `dyn` or across threads\"\n)]\n",
    );
    output.push_str(&format!("pub trait {trait_name} {{\n"));
    push_doc(&mut output, 4, &["The pool reads and transactions use."]);
    output.push_str("    fn pool(&self) -> &PgPool;\n");
    for endpoint in &feature.endpoints {
        output.push('\n');
        match endpoint {
            Endpoint::List => render_list(&mut output, feature),
            Endpoint::GetOne => render_get_one(&mut output, feature),
            Endpoint::Create | Endpoint::Update | Endpoint::Remove => {
                render_write(&mut output, feature, *endpoint);
            }
        }
    }
    output.push_str("}\n");
    if feature.key.is_some() {
        output.push_str(&format!(
            "\n/// The `404` of a key no `{}` row has.\nfn not_found() -> AppError {{\n    AppError::NotFound(\"no `{}` row has this key\".to_owned())\n}}\n",
            feature.table.display_name(),
            feature.table.display_name(),
        ));
    }
    output
}

fn imports(feature: &Feature<'_>) -> String {
    let writes = feature.endpoints.iter().any(|endpoint| endpoint.writes());
    let mut sqlx = vec!["PgPool"];
    if writes {
        sqlx.insert(0, "PgConnection");
    }
    let mut dto = Vec::new();
    if feature.serves(Endpoint::Create) {
        dto.push(feature.create_dto());
    }
    if feature.serves(Endpoint::List) {
        dto.push(feature.list_query());
        dto.push(feature.sort_enum());
    }
    if feature.serves(Endpoint::Update) {
        dto.push(feature.update_dto());
    }
    let mut model = vec![feature.row().to_owned()];
    if feature
        .endpoints
        .iter()
        .any(|endpoint| endpoint.takes_key())
    {
        model.extend(feature.key_type());
    }
    model.sort();
    let mut output = String::new();
    push_use(&mut output, "sqlx", &sqlx);
    output.push('\n');
    dto.sort();
    let dto: Vec<&str> = dto.iter().map(String::as_str).collect();
    push_use(&mut output, "super::dto", &dto);
    let model: Vec<&str> = model.iter().map(String::as_str).collect();
    push_use(&mut output, "super::model", &model);
    output.push_str("use super::repo;\n");
    if feature.serves(Endpoint::List) {
        output.push_str(&format!(
            "use {}::search::{{Direction, Sort}};\n",
            feature.db_path
        ));
    }
    output.push_str("use crate::error::AppError;\n");
    if feature.serves(Endpoint::List) {
        output.push_str("use crate::shared::response::PaginatedResponse;\n");
    }
    output.push('\n');
    output
}

/// The key argument the data layer takes: the key, or a borrow of it.
fn key_arg(feature: &Feature<'_>) -> String {
    let param = crate::generator::keys::key_param_type(feature.table);
    let binding = key_binding(feature);
    if param.starts_with('&') {
        format!("&{binding}")
    } else {
        binding
    }
}

/// The name the service takes the key by: the key column's, or `key`.
fn key_binding(feature: &Feature<'_>) -> String {
    match &feature.key {
        Some(Key::Single(column)) => crate::generator::param_name(column),
        _ => "key".to_owned(),
    }
}

/// The key parameter, such as `id: NoteId`.
fn key_param(feature: &Feature<'_>) -> String {
    format!(
        "{}: {}",
        key_binding(feature),
        feature.key_type().unwrap_or_default()
    )
}

fn render_list(output: &mut String, feature: &Feature<'_>) {
    let page = format!("PaginatedResponse<{}>", feature.row());
    push_doc(
        output,
        4,
        &[&format!(
            "`GET {}`: one page of rows, sorted and matched by `query`.",
            feature.url
        )],
    );
    push_signature(
        output,
        4,
        "async fn list",
        &[
            "&self".to_owned(),
            format!("query: {}", feature.list_query()),
        ],
        &format!("Result<{page}, AppError>"),
    );
    output.push_str("        let pagination = query.pagination();\n");
    output.push_str("        let direction = Direction::from(query.sort_order());\n");
    output.push_str(&format!(
        "        let default = {}::DEFAULT;\n        let key = query.sort.unwrap_or(default);\n",
        feature.sort_enum()
    ));
    output.push_str("        let sort = Some(Sort { key, direction });\n");
    let q = !feature.search.is_empty();
    if q {
        output.push_str("        let q = query.q.as_deref().filter(|q| !q.is_empty());\n");
    }
    let q_arg = if q { "q, " } else { "" };
    output.push_str(&format!(
        "        let rows = repo::list_matching(self.pool(), {q_arg}sort, pagination.into()).await?;\n"
    ));
    let count_args = if q { "self.pool(), q" } else { "self.pool()" };
    output.push_str(&format!(
        "        let total = repo::count_matching({count_args}).await?;\n"
    ));
    output.push_str("        Ok(PaginatedResponse::new(rows, pagination, total))\n    }\n");
}

fn render_get_one(output: &mut String, feature: &Feature<'_>) {
    push_doc(
        output,
        4,
        &[&format!(
            "`GET {}{}`: the row with this key, or `404`.",
            feature.url,
            feature.key_path()
        )],
    );
    push_signature(
        output,
        4,
        "async fn get_one",
        &["&self".to_owned(), key_param(feature)],
        &format!("Result<{}, AppError>", feature.row()),
    );
    output.push_str(&format!(
        "        let row = repo::get_by_id(self.pool(), {}).await?;\n        row.ok_or_else(not_found)\n    }}\n",
        key_arg(feature)
    ));
}

/// `<write>_in` and `<write>`.
fn render_write(output: &mut String, feature: &Feature<'_>, endpoint: Endpoint) {
    let name = endpoint.name();
    let (doc, inputs, returns) = match endpoint {
        Endpoint::Create => (
            format!("`POST {}`: creates a row and returns it.", feature.url),
            vec![format!("input: {}", feature.create_dto())],
            feature.row().to_owned(),
        ),
        Endpoint::Update => (
            format!(
                "`PATCH {}{}`: changes the row with this key and returns it, or `404`.",
                feature.url,
                feature.key_path()
            ),
            vec![
                key_param(feature),
                format!("input: {}", feature.update_dto()),
            ],
            feature.row().to_owned(),
        ),
        _ => (
            format!(
                "`DELETE {}{}`: deletes the row with this key, or `404`.",
                feature.url,
                feature.key_path()
            ),
            vec![key_param(feature)],
            "()".to_owned(),
        ),
    };
    let returns = format!("Result<{returns}, AppError>");

    push_doc(
        output,
        4,
        &[
            &format!("What [`{name}`](Self::{name}) does, on `conn`: the caller's connection or"),
            "transaction. Override this one to change it.",
        ],
    );
    let mut params = vec!["&self".to_owned(), "conn: &mut PgConnection".to_owned()];
    params.extend(inputs.iter().cloned());
    push_signature(output, 4, &format!("async fn {name}_in"), &params, &returns);
    match endpoint {
        Endpoint::Create => {
            output.push_str("        Ok(repo::create(conn, input.into()).await?)\n");
        }
        Endpoint::Update => output.push_str(&format!(
            "        let row = repo::update(conn, {}, input.into()).await?;\n        row.ok_or_else(not_found)\n",
            key_arg(feature)
        )),
        _ => output.push_str(&format!(
            "        if repo::delete(conn, {}).await? {{\n            Ok(())\n        }} else {{\n            Err(not_found())\n        }}\n",
            key_arg(feature)
        )),
    }
    output.push_str("    }\n\n");

    push_doc(
        output,
        4,
        &[
            &doc,
            &format!("Runs [`{name}_in`](Self::{name}_in) in a transaction of its own."),
        ],
    );
    let mut params = vec!["&self".to_owned()];
    params.extend(inputs.iter().cloned());
    push_signature(output, 4, &format!("async fn {name}"), &params, &returns);
    let args: Vec<&str> = inputs
        .iter()
        .map(|input| input.split(':').next().unwrap_or_default())
        .collect();
    output.push_str("        let mut tx = self.pool().begin().await?;\n");
    let call = format!("self.{name}_in(&mut tx, {}).await?;", args.join(", "));
    if endpoint == Endpoint::Remove {
        output.push_str(&format!("        {call}\n"));
        output.push_str("        tx.commit().await?;\n        Ok(())\n    }\n");
    } else {
        output.push_str(&format!("        let row = {call}\n"));
        output.push_str("        tx.commit().await?;\n        Ok(row)\n    }\n");
    }
}

/// Renders the scaffolded `service.rs`.
pub fn render_scaffold(feature: &Feature<'_>) -> String {
    let service = feature.service();
    let trait_name = feature.service_api();
    format!(
        "{SCAFFOLD_HEADER}

//! `{service}`: the `{table}` feature's business rules. Override any
//! [`{trait_name}`] method here; the others keep their generated defaults.
//! A port this feature needs from another feature is a trait declared here
//! and implemented in `app.rs`.

use sqlx::PgPool;

use super::service_api::{trait_name};

/// The `{table}` service, registered as app data by `register_services`.
pub struct {service} {{
    /// The pool the service reads and writes through.
    pub pool: PgPool,
}}

impl {service} {{
    /// The service over `pool`, as `register_services` builds it. Give it
    /// what else it needs (a port) here.
    #[must_use]
    pub fn new(pool: PgPool) -> Self {{
        {service} {{ pool }}
    }}
}}

impl {trait_name} for {service} {{
    fn pool(&self) -> &PgPool {{
        &self.pool
    }}
}}
",
        table = feature.table.display_name(),
    )
}
