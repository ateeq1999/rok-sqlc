//! A feature's service (ADR 0016): `service_api.rs`, the generated trait
//! with one method per endpoint, each defaulting to the data layer, and
//! `service.rs`, the scaffolded struct implementing it, which the user owns.

use super::feature::{Endpoint, Feature, Key};
use super::layout::{Uses, push_call, push_signature};
use super::relation_service;
use super::scope::tenant_arg;
use crate::generator::{HEADER, push_doc};
use crate::output::SCAFFOLD_HEADER;

/// Renders `service_api.rs`.
pub fn render_api(feature: &Feature<'_>) -> String {
    let trait_name = feature.service_api();
    let mut uses = Uses::default();
    uses.add("sqlx::PgPool");
    uses.add(&format!("super::model::{}", feature.row()));
    uses.add("super::repo");
    uses.add("crate::error::AppError");
    let mut body = String::new();
    for endpoint in &feature.endpoints {
        body.push('\n');
        match endpoint {
            Endpoint::List => render_list(&mut body, &mut uses, feature),
            Endpoint::Search => super::search::render_service(&mut body, &mut uses, feature),
            Endpoint::GetOne => render_get_one(&mut body, &mut uses, feature),
            Endpoint::Create | Endpoint::Update | Endpoint::Remove | Endpoint::Restore => {
                render_write(&mut body, &mut uses, feature, *endpoint);
            }
        }
    }
    for lookup in &feature.lookups {
        body.push('\n');
        render_lookup(&mut body, &mut uses, feature, lookup);
    }
    relation_service::render_service(&mut body, &mut uses, feature);
    if feature.takes_user() {
        uses.add("crate::shared::auth::AuthUser");
    }

    let mut output = format!(
        "{HEADER}\n//! [`{trait_name}`]: the `{}` feature's operations, one per endpoint,\n//! each with a working default over the data layer.\n\n",
        feature.table.display_name()
    );
    output.push_str(&uses.render());
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
    output.push_str(&body);
    output.push_str("}\n");
    // Each error helper only when a method calls it: `-D warnings` rejects
    // dead code, and a list-only feature reads no row by key.
    if calls(&body, "not_found") {
        output.push_str(&format!(
            "\n/// The `404` of a key no `{}` row has.\nfn not_found() -> AppError {{\n    AppError::NotFound(\"no `{}` row has this key\".to_owned())\n}}\n",
            feature.table.display_name(),
            feature.table.display_name(),
        ));
    }
    if calls(&body, "not_found_in") {
        output.push_str(
            "\n/// The `404` naming a related `table`: no row of it the caller may see has\n/// this key, or none is paired with the row.\nfn not_found_in(table: &str) -> AppError {\n    AppError::NotFound(format!(\"no `{table}` row has this key\"))\n}\n",
        );
    }
    if calls(&body, "stale") {
        output.push_str(&format!(
            "\n/// The `412` of an `If-Match` that isn't the `{}` row's version.\nfn stale() -> AppError {{\n    AppError::VersionConflict(\"the row has changed since this version\".to_owned())\n}}\n",
            feature.table.display_name(),
        ));
    }
    output
}

/// Whether the generated `body` calls the function `name`: `name(...)`, or
/// `name` handed to `ok_or_else`.
fn calls(body: &str, name: &str) -> bool {
    body.match_indices(name).any(|(at, _)| {
        let before = body[..at].chars().next_back();
        let after = body[at + name.len()..].chars().next();
        !before.is_some_and(|c| c.is_alphanumeric() || c == '_') && matches!(after, Some('(' | ')'))
    })
}

/// The key argument the data layer takes: the key, or a borrow of it.
pub fn key_arg(feature: &Feature<'_>) -> String {
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
pub fn key_param(feature: &Feature<'_>) -> String {
    format!(
        "{}: {}",
        key_binding(feature),
        feature.key_type().unwrap_or_default()
    )
}

/// The parameters of an endpoint's method: `&self`, the tenant, and the
/// caller of an owned feature.
pub fn own_params(feature: &Feature<'_>) -> Vec<String> {
    let mut params = vec!["&self".to_owned()];
    params.extend(feature.scope_params(feature.tenant, feature.owner.is_some()));
    params
}

/// The leading arguments of a data layer call on the feature's rows:
/// `executor` and the tenant.
pub fn data_args(feature: &Feature<'_>, executor: &str) -> Vec<String> {
    let mut args = vec![executor.to_owned()];
    args.extend(
        tenant_arg(feature.table)
            .strip_suffix(", ")
            .map(str::to_owned),
    );
    args
}

/// `if !<binding> { return Err(not_found()); }`.
pub fn push_not_owned(output: &mut String, binding: &str) {
    output.push_str(&format!(
        "        if !{binding} {{\n            return Err(not_found());\n        }}\n"
    ));
}

fn render_list(output: &mut String, uses: &mut Uses, feature: &Feature<'_>) {
    uses.add(&format!("super::dto::{}", feature.list_query()));
    uses.add(&format!("super::dto::{}", feature.sort_enum()));
    uses.add(&format!("{}::search::Direction", feature.db_path));
    uses.add(&format!("{}::search::Sort", feature.db_path));
    uses.add("crate::shared::response::PaginatedResponse");
    let page = format!("PaginatedResponse<{}>", feature.row());
    push_doc(
        output,
        4,
        &[&format!(
            "`GET {}`: one page of rows, sorted and matched by `query`.",
            feature.url
        )],
    );
    let mut params = own_params(feature);
    params.push(format!("query: {}", feature.list_query()));
    push_signature(
        output,
        4,
        "async fn list",
        &params,
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
    let mut args = data_args(feature, "self.pool()");
    if feature.owner.is_some() {
        args.push("user.id".to_owned());
    }
    if q {
        args.push("q".to_owned());
    }
    let mut list_args = args.clone();
    list_args.extend(["sort".to_owned(), "pagination.into()".to_owned()]);
    push_call(
        output,
        8,
        "let rows = ",
        "repo::list_matching",
        &list_args,
        ".await?;",
    );
    push_call(
        output,
        8,
        "let total = ",
        "repo::count_matching",
        &args,
        ".await?;",
    );
    output.push_str("        Ok(PaginatedResponse::new(rows, pagination, total))\n    }\n");
}

fn render_get_one(output: &mut String, uses: &mut Uses, feature: &Feature<'_>) {
    uses.add(&format!(
        "super::model::{}",
        feature.key_type().unwrap_or_default()
    ));
    push_doc(
        output,
        4,
        &[&format!(
            "`GET {}{}`: the row with this key, or `404`.",
            feature.url,
            feature.key_path()
        )],
    );
    let mut params = own_params(feature);
    params.push(key_param(feature));
    push_signature(
        output,
        4,
        "async fn get_one",
        &params,
        &format!("Result<{}, AppError>", feature.row()),
    );
    let mut args = data_args(feature, "self.pool()");
    args.push(key_arg(feature));
    let callee = if feature.owner.is_some() {
        args.push("user.id".to_owned());
        "repo::get_owned"
    } else {
        "repo::get_by_id"
    };
    push_call(output, 8, "let row = ", callee, &args, ".await?;");
    output.push_str("        row.ok_or_else(not_found)\n    }\n");
}

fn render_lookup(
    output: &mut String,
    uses: &mut Uses,
    feature: &Feature<'_>,
    lookup: &super::feature::Lookup<'_>,
) {
    super::scope::import_type(feature, uses, lookup.column);
    push_doc(
        output,
        4,
        &[&format!(
            "`GET {}{}`: the row whose `{}` is `value`, or `404`.",
            feature.url,
            lookup.path(),
            lookup.column.sql_name
        )],
    );
    let mut params = own_params(feature);
    params.push(format!("value: {}", lookup.column.value_type()));
    let head = format!("async fn {}", lookup.function);
    let returns = format!("Result<{}, AppError>", feature.row());
    push_signature(output, 4, &head, &params, &returns);
    let mut args = data_args(feature, "self.pool()");
    let borrowed = crate::generator::param_type(lookup.column).starts_with('&');
    args.push(if borrowed { "&value" } else { "value" }.to_owned());
    let callee = format!("repo::{}", lookup.function);
    push_call(output, 8, "let row = ", &callee, &args, ".await?;");
    if feature.owner.is_some() {
        output.push_str("        let row = row.filter(|row| repo::owned_by(row, user.id));\n");
    }
    output.push_str("        row.ok_or_else(not_found)\n    }\n");
}

/// An owned feature's check, on `conn`, that the caller owns the row with
/// the key, locking it until the transaction ends: `404` otherwise.
fn push_owned_check(output: &mut String, feature: &Feature<'_>) {
    if feature.owner.is_some() {
        let mut args = data_args(feature, "&mut *conn");
        args.extend([key_arg(feature), "user.id".to_owned()]);
        push_call(
            output,
            8,
            "let owned = ",
            "repo::lock_owned",
            &args,
            ".await?;",
        );
        push_not_owned(output, "owned");
    }
}

/// `<write>_in` and `<write>`, and for a soft-delete table's `remove`, the
/// pair of `remove_permanently`.
fn render_write(output: &mut String, uses: &mut Uses, feature: &Feature<'_>, endpoint: Endpoint) {
    uses.add("sqlx::PgConnection");
    if endpoint.takes_key() {
        uses.add(&format!(
            "super::model::{}",
            feature.key_type().unwrap_or_default()
        ));
    }
    let key_path = feature.key_path();
    let row = feature.row().to_owned();
    let (doc, inputs, returns) = match endpoint {
        Endpoint::Create => {
            uses.add(&format!("super::dto::{}", feature.create_dto()));
            (
                format!("`POST {}`: creates a row and returns it.", feature.url),
                vec![format!("input: {}", feature.create_dto())],
                row,
            )
        }
        Endpoint::Update => {
            uses.add(&format!("super::dto::{}", feature.update_dto()));
            let mut inputs = vec![key_param(feature)];
            let mut doc = format!(
                "`PATCH {}{key_path}`: changes the row with this key and returns it, or `404`.",
                feature.url
            );
            if let Some(version) = feature.version {
                uses.add(&format!("{}::UpdateOutcome", feature.db_path));
                inputs.push(format!("version: {}", version.value_type()));
                doc = format!(
                    "`PATCH {}{key_path}`: changes the row with this key if it is at `version` and\nreturns it; `404` if there is none, `412` if it is at another version.",
                    feature.url
                );
            }
            inputs.push(format!("input: {}", feature.update_dto()));
            (doc, inputs, row)
        }
        Endpoint::Restore => (
            format!(
                "`POST {}{key_path}/restore`: restores the soft-deleted row with this key and\nreturns it, or `404`.",
                feature.url
            ),
            vec![key_param(feature)],
            row,
        ),
        _ => (
            format!(
                "`DELETE {}{key_path}`: {} the row with this key, or `404`.",
                feature.url,
                if feature.soft_delete {
                    "soft-deletes"
                } else {
                    "deletes"
                }
            ),
            vec![key_param(feature)],
            "()".to_owned(),
        ),
    };
    let mut params = own_params(feature);
    params.extend(inputs);
    let inputs = params[1..].to_vec();
    let name = endpoint.name();
    render_write_pair(output, feature, name, endpoint, &doc, &inputs, &returns);
    if endpoint == Endpoint::Remove && feature.soft_delete {
        let doc = format!(
            "`DELETE {}{key_path}?permanent=true`: deletes the row with this key for good,\nsoft-deleted or not, or `404`.",
            feature.url
        );
        output.push('\n');
        render_write_pair(
            output,
            feature,
            "remove_permanently",
            endpoint,
            &doc,
            &inputs,
            &returns,
        );
    }
}

/// `<name>_in`, holding the logic, and `<name>`, its transaction.
fn render_write_pair(
    output: &mut String,
    feature: &Feature<'_>,
    name: &str,
    endpoint: Endpoint,
    doc: &str,
    inputs: &[String],
    returns: &str,
) {
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
    if endpoint != Endpoint::Create {
        push_owned_check(output, feature);
    }
    let mut args = data_args(feature, "&mut *conn");
    match endpoint {
        Endpoint::Create => {
            args.push(if feature.owner.is_some() {
                "input.into_new(user.id)".to_owned()
            } else {
                "input.into()".to_owned()
            });
            push_call(output, 8, "let row = ", "repo::create", &args, ".await?;");
            output.push_str("        Ok(row)\n");
        }
        Endpoint::Update => {
            args.push(key_arg(feature));
            if feature.version.is_some() {
                args.extend(["version".to_owned(), "input.into()".to_owned()]);
                push_call(
                    output,
                    8,
                    "let outcome = ",
                    "repo::update",
                    &args,
                    ".await?;",
                );
                output.push_str(
                    "        match outcome {\n            UpdateOutcome::Updated(row) => Ok(row),\n            UpdateOutcome::NotFound => Err(not_found()),\n            UpdateOutcome::VersionConflict => Err(stale()),\n        }\n",
                );
            } else {
                args.push("input.into()".to_owned());
                push_call(output, 8, "let row = ", "repo::update", &args, ".await?;");
                output.push_str("        row.ok_or_else(not_found)\n");
            }
        }
        Endpoint::Restore => {
            let mut restore = args.clone();
            restore.push(key_arg(feature));
            push_call(
                output,
                8,
                "let restored = ",
                "repo::restore",
                &restore,
                ".await?;",
            );
            push_not_owned(output, "restored");
            args.push(key_arg(feature));
            push_call(
                output,
                8,
                "let row = ",
                "repo::get_by_id",
                &args,
                ".await?;",
            );
            output.push_str("        row.ok_or_else(not_found)\n");
        }
        _ => {
            args.push(key_arg(feature));
            let callee = if name == "remove_permanently" {
                "repo::delete_permanently"
            } else {
                "repo::delete"
            };
            push_call(output, 8, "let deleted = ", callee, &args, ".await?;");
            output.push_str("        if deleted { Ok(()) } else { Err(not_found()) }\n");
        }
    }
    output.push_str("    }\n\n");
    let runs = format!("Runs [`{name}_in`](Self::{name}_in) in a transaction of its own.");
    let mut lines: Vec<&str> = doc.lines().collect();
    lines.push(&runs);
    push_doc(output, 4, &lines);
    let returns = returns
        .trim_start_matches("Result<")
        .trim_end_matches(", AppError>")
        .to_owned();
    push_transaction(output, name, inputs, &returns);
}

/// `<name>`: runs `<name>_in` with `inputs` in a transaction of its own,
/// returning what it does (`returns`).
pub fn push_transaction(output: &mut String, name: &str, inputs: &[String], returns: &str) {
    let mut params = vec!["&self".to_owned()];
    params.extend(inputs.iter().cloned());
    let result = format!("Result<{returns}, AppError>");
    push_signature(output, 4, &format!("async fn {name}"), &params, &result);
    let mut args = vec!["&mut tx".to_owned()];
    args.extend(
        inputs
            .iter()
            .map(|input| input.split(':').next().unwrap_or_default().to_owned()),
    );
    output.push_str("        let mut tx = self.pool().begin().await?;\n");
    let callee = format!("self.{name}_in");
    if returns == "()" {
        push_call(output, 8, "", &callee, &args, ".await?;");
        output.push_str("        tx.commit().await?;\n        Ok(())\n    }\n");
    } else {
        push_call(output, 8, "let row = ", &callee, &args, ".await?;");
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

{impl_head}    fn pool(&self) -> &PgPool {{
        &self.pool
    }}
}}
",
        table = feature.table.display_name(),
        impl_head = super::layout::impl_head(&trait_name, &service),
    )
}
