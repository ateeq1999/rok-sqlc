//! A feature's `dto.rs`: the list's query string, the create and update
//! bodies (validated by the same rules as the data layer's input and patch),
//! and the response, joined to the data layer's types by `From`.

use super::feature::{Endpoint, Feature};
use super::layout::{Uses, push_field_attr, push_struct_lit};
use super::{relation_routes, search};
use crate::generator::openapi::{push_into_params, push_to_schema, push_value_type, value_type};
use crate::generator::validate::{push_decimal_checks, push_field_rule, push_rule};
use crate::generator::{
    HEADER, MAX_WIDTH, doc_name, insert_field_type, push_doc, update_field_type,
};
use crate::schema::{Column, mentions_generated_type};

/// Renders `dto.rs`.
pub fn render(feature: &Feature<'_>) -> String {
    let table = feature.table;
    let create = feature
        .serves(Endpoint::Create)
        .then(|| create_columns(feature));
    let update = feature
        .serves(Endpoint::Update)
        .then(|| update_columns(feature));
    let mut output = format!(
        "{HEADER}\n//! The `{}` request and response bodies.\n\n",
        table.display_name()
    );

    let mut fields: Vec<&Column> = feature.response_columns();
    fields.extend(create.iter().flatten());
    fields.extend(update.iter().flatten());
    let mut uses = Uses::default();
    let mut body = String::new();
    if feature.serves(Endpoint::List) {
        render_list_query(&mut body, feature);
    }
    if feature.serves(Endpoint::Search) {
        fields.extend(search::render_dto(&mut body, &mut uses, feature));
    }
    if let Some(columns) = &create {
        render_create(&mut body, feature, columns);
    }
    if let Some(columns) = &update {
        render_update(&mut body, feature, columns);
    }
    if feature.soft_delete && feature.serves(Endpoint::Remove) {
        render_remove_query(&mut body, feature);
    }
    render_response(&mut body, feature);
    relation_routes::render_dto(&mut body, &mut uses, feature);
    let checked = create.iter().flatten().chain(update.iter().flatten());
    push_decimal_checks(&mut body, checked.copied());

    imports(
        &mut uses,
        feature,
        &fields,
        create.is_some(),
        update.is_some(),
    );
    output.push_str(&uses.render());
    if feature.serves(Endpoint::List) || feature.serves(Endpoint::Search) {
        output.push_str(&format!(
            "pub use {}::listing::{};\n\n",
            feature.db_module(),
            feature.sort_enum()
        ));
    }
    output.push_str(body.trim_start_matches('\n'));
    output
}

/// `DELETE`'s query string on a soft-delete table.
fn render_remove_query(output: &mut String, feature: &Feature<'_>) {
    output.push('\n');
    push_doc(
        output,
        0,
        &[&format!(
            "`DELETE {}{}`'s query string: `?permanent=true` deletes the row for good.",
            feature.url,
            feature.key_path()
        )],
    );
    output.push_str("#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Deserialize)]\n");
    push_into_params(output, feature.openapi);
    output.push_str(&format!(
        "pub struct {} {{\n    /// Whether to delete the row for good rather than soft-delete it.\n    #[serde(default)]\n    pub permanent: bool,\n}}\n",
        feature.remove_query()
    ));
}

/// The create body's columns: the create input's, less a key the server
/// makes (`primary_key_ids`) and the owner, which is the caller.
fn create_columns<'a>(feature: &Feature<'a>) -> Vec<&'a Column> {
    feature
        .table
        .insert_columns()
        .into_iter()
        .filter(|column| column.id_generation.is_none() && !is_owner(feature, column))
        .collect()
}

/// The update body's columns: the patch's, less the owner and the immutable
/// columns, which no update changes.
fn update_columns<'a>(feature: &Feature<'a>) -> Vec<&'a Column> {
    feature
        .table
        .update_columns()
        .into_iter()
        .filter(|column| !is_owner(feature, column) && !feature.is_immutable(column))
        .collect()
}

/// The patch's immutable columns, which the update body rejects by name.
fn rejected_columns<'a>(feature: &Feature<'a>) -> Vec<&'a Column> {
    feature
        .table
        .update_columns()
        .into_iter()
        .filter(|column| feature.is_immutable(column))
        .collect()
}

/// Whether `column` is the feature's owner column.
fn is_owner(feature: &Feature<'_>, column: &Column) -> bool {
    feature
        .owner
        .is_some_and(|owner| owner.sql_name == column.sql_name)
}

/// The value of the owner field of the create input, built from `owner`, a
/// `uuid::Uuid`: converted to its typed id, and wrapped in `Some` when the
/// input's field is optional.
fn owner_value(owner: &Column) -> String {
    let value = if owner.typed_id.is_some() {
        "owner.into()"
    } else {
        "owner"
    };
    if insert_field_type(owner).starts_with("Option<") {
        format!("Some({value})")
    } else {
        value.to_owned()
    }
}

/// The `use` lines: the data layer's types the bodies convert to and from,
/// the typed ids and enums their fields hold, and the list's query.
fn imports(uses: &mut Uses, feature: &Feature<'_>, fields: &[&Column], create: bool, update: bool) {
    let table = feature.table;
    uses.add(&format!("super::model::{}", feature.row()));
    if create {
        uses.add(&format!("super::model::New{}", feature.row()));
    }
    if update {
        uses.add(&format!("super::model::Update{}", feature.row()));
    }
    for id in fields.iter().filter_map(|column| column.typed_id.as_ref()) {
        if id.module == table.module_name {
            uses.add(&format!("super::model::{}", id.name));
        } else {
            uses.add(&format!(
                "{}::{}::models::{}",
                feature.db_path, id.module, id.name
            ));
        }
    }
    if feature.serves(Endpoint::List) {
        uses.add(&format!(
            "crate::shared::pagination::{}",
            list_query_base(feature)
        ));
    }
    if fields
        .iter()
        .any(|column| mentions_generated_type(&column.rust_type))
    {
        uses.add(&format!("{}::types", feature.db_path));
    }
}

/// The shared query string a list takes: `ListQuery`, with `q`, when the
/// table has search columns, otherwise `PageQuery`, which neither takes
/// nor documents `q`.
fn list_query_base(feature: &Feature<'_>) -> &'static str {
    if feature.search.is_empty() {
        "PageQuery"
    } else {
        "ListQuery"
    }
}

fn render_list_query(output: &mut String, feature: &Feature<'_>) {
    let mut doc = Vec::new();
    if feature.search.is_empty() {
        doc.push(format!(
            "`GET {}`'s query string: `?page=&per_page=&sort=&order=`.",
            feature.url
        ));
    } else {
        doc.push(format!(
            "`GET {}`'s query string: `?page=&per_page=&q=&sort=&order=`.",
            feature.url
        ));
        let names: Vec<String> = feature
            .search
            .iter()
            .map(|column| doc_name(&column.sql_name))
            .collect();
        doc.push(format!("`q` matches {}.", names.join(", ")));
    }
    doc.push(format!(
        "`sort` takes a [`{}`], `{}` unless given.",
        feature.sort_enum(),
        feature
            .default_sort()
            .map(|column| column.sql_name.clone())
            .unwrap_or_default()
    ));
    let doc: Vec<&str> = doc.iter().map(String::as_str).collect();
    push_doc(output, 0, &doc);
    let alias = format!("pub type {} =", feature.list_query());
    let target = format!("{}<{}>;", list_query_base(feature), feature.sort_enum());
    if alias.len() + 1 + target.len() <= MAX_WIDTH {
        output.push_str(&format!("{alias} {target}\n"));
    } else {
        output.push_str(&format!("{alias}\n    {target}\n"));
    }
}

/// The doc line of a body field.
fn field_doc(column: &Column) -> String {
    format!("The {} column.", doc_name(&column.sql_name))
}

/// The `#[serde(...)]` of an `Option<Option<T>>` field: `null` is
/// `Some(None)`, through the data layer's `nullable` helper.
fn push_nullable(output: &mut String, feature: &Feature<'_>, default: bool) {
    let mut args = Vec::new();
    if default {
        args.push("default".to_owned());
    }
    args.push(format!(
        "deserialize_with = \"{}::nullable::deserialize\"",
        feature.db_path
    ));
    push_field_attr(output, "serde", &args);
}

/// `impl From<{from}> for {to}`, building `to` from `binding`'s `fields`.
fn push_from(output: &mut String, from: &str, to: &str, binding: &str, fields: &[String]) {
    output.push('\n');
    output.push_str(&super::layout::impl_head(&format!("From<{from}>"), to));
    output.push_str(&format!("    fn from({binding}: {from}) -> Self {{\n"));
    push_struct_lit(output, 8, to, fields);
    output.push_str("    }\n}\n");
}

fn render_create(output: &mut String, feature: &Feature<'_>, columns: &[&Column]) {
    let dto = feature.create_dto();
    let new = format!("New{}", feature.row());
    output.push('\n');
    push_doc(
        output,
        0,
        &[
            &format!(
                "`POST {}`'s body, checked by the rules of `{new}`.",
                feature.url
            ),
            "A field for a defaulted column may be left out.",
        ],
    );
    output
        .push_str("#[derive(Debug, Clone, PartialEq, serde::Deserialize, validator::Validate)]\n");
    push_to_schema(output, feature.openapi);
    output.push_str(&format!("pub struct {dto} {{\n"));
    for column in columns {
        push_doc(output, 4, &[&field_doc(column)]);
        if column.is_omittable_on_insert() && column.nullable {
            push_nullable(output, feature, true);
        }
        push_field_rule(output, column);
        push_field(output, feature, column, &insert_field_type(column));
    }
    output.push_str("}\n");
    let binding = if feature.owner.is_some() {
        "self"
    } else {
        "dto"
    };
    let fields: Vec<String> = feature
        .table
        .insert_columns()
        .iter()
        .map(|column| match column.id_generation {
            Some(_) => format!("{}: None", column.rust_name),
            None if is_owner(feature, column) => {
                format!("{}: {}", column.rust_name, owner_value(column))
            }
            None => format!("{name}: {binding}.{name}", name = column.rust_name),
        })
        .collect();
    if feature.owner.is_none() {
        push_from(output, &dto, &new, "dto", &fields);
        return;
    }
    // The owner is the caller, which only the service knows.
    output.push_str(&format!("\nimpl {dto} {{\n"));
    push_doc(
        output,
        4,
        &[&format!(
            "The [`{new}`] of this body, owned by `owner`: the caller's id."
        )],
    );
    output.push_str(&format!(
        "    #[must_use]\n    pub fn into_new(self, owner: uuid::Uuid) -> {new} {{\n"
    ));
    push_struct_lit(output, 8, &new, &fields);
    output.push_str("    }\n}\n");
}

fn render_update(output: &mut String, feature: &Feature<'_>, columns: &[&Column]) {
    let dto = feature.update_dto();
    let patch = format!("Update{}", feature.row());
    output.push('\n');
    push_doc(
        output,
        0,
        &[
            &format!(
                "`PATCH {}{}`'s body, checked by the rules of `{patch}`.",
                feature.url,
                feature.key_path()
            ),
            "Every field is optional: a field left out keeps its column's value, and",
            "`null` sets a nullable column to `NULL`.",
        ],
    );
    output.push_str(
        "#[derive(Debug, Clone, Default, PartialEq, serde::Deserialize, validator::Validate)]\n",
    );
    push_to_schema(output, feature.openapi);
    output.push_str("#[serde(default)]\n");
    output.push_str(&format!("pub struct {dto} {{\n"));
    for column in columns {
        push_doc(
            output,
            4,
            &[&format!(
                "The new value of the {} column.",
                doc_name(&column.sql_name)
            )],
        );
        if column.nullable {
            push_nullable(output, feature, false);
        }
        push_field_rule(output, column);
        push_field(output, feature, column, &update_field_type(column));
    }
    let rejected = rejected_columns(feature);
    for column in &rejected {
        push_doc(
            output,
            4,
            &[&format!(
                "The {} column is immutable: naming it, even as `null`, is a `422`.",
                doc_name(&column.sql_name)
            )],
        );
        if feature.openapi {
            output.push_str("    #[schema(ignore)]\n");
        }
        output.push_str("    #[serde(deserialize_with = \"present\")]\n");
        push_rule(
            output,
            "custom",
            &[
                "function = \"immutable\"".to_owned(),
                format!("message = \"`{}` can't be changed\"", column.sql_name),
            ],
        );
        output.push_str(&format!(
            "    pub {}: Option<serde_json::Value>,\n",
            column.rust_name
        ));
    }
    output.push_str("}\n");
    let mut fields: Vec<String> = columns
        .iter()
        .map(|column| format!("{name}: dto.{name}", name = column.rust_name))
        .collect();
    // No update changes the owner or an immutable column.
    let unchanged = feature.owner.into_iter().chain(rejected.iter().copied());
    fields.extend(unchanged.map(|column| format!("{}: None", column.rust_name)));
    push_from(output, &dto, &patch, "dto", &fields);
    if !rejected.is_empty() {
        output.push('\n');
        push_doc(
            output,
            0,
            &["Deserializes a field that is present, `null` included, as `Some`."],
        );
        output.push_str(
            "fn present<'de, D>(deserializer: D) -> Result<Option<serde_json::Value>, D::Error>\nwhere\n    D: serde::Deserializer<'de>,\n{\n    serde::Deserialize::deserialize(deserializer).map(Some)\n}\n",
        );
        output.push('\n');
        push_doc(
            output,
            0,
            &["Rejects a field naming an immutable column: no update changes one."],
        );
        output.push_str(
            "fn immutable(_: &serde_json::Value) -> Result<(), validator::ValidationError> {\n    Err(validator::ValidationError::new(\"immutable\"))\n}\n",
        );
    }
}

fn render_response(output: &mut String, feature: &Feature<'_>) {
    let response = feature.response();
    let row = feature.row();
    output.push('\n');
    push_doc(
        output,
        0,
        &[&format!(
            "A `{}` row as the API answers it.",
            feature.table.display_name()
        )],
    );
    output.push_str("#[derive(Debug, Clone, PartialEq, serde::Serialize)]\n");
    push_to_schema(output, feature.openapi);
    output.push_str(&format!("pub struct {response} {{\n"));
    let columns = feature.response_columns();
    for column in &columns {
        push_doc(output, 4, &[&field_doc(column)]);
        push_field(output, feature, column, &column.field_type());
    }
    output.push_str("}\n");
    let fields: Vec<String> = columns
        .iter()
        .map(|column| format!("{name}: row.{name}", name = column.rust_name))
        .collect();
    push_from(output, row, &response, "row", &fields);
}

/// A body field holding `column` as `field_type`, with the `value_type`
/// the OpenAPI document needs when utoipa can't describe `field_type`.
fn push_field(output: &mut String, feature: &Feature<'_>, column: &Column, field_type: &str) {
    push_value_type(output, feature.openapi, value_type(column, field_type));
    output.push_str(&format!("    pub {}: {field_type},\n", column.rust_name));
}
