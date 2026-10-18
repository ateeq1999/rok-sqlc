//! A feature's `dto.rs`: the list's query string, the create and update
//! bodies (validated by the same rules as the data layer's input and patch),
//! and the response, joined to the data layer's types by `From`.

use super::feature::{Endpoint, Feature};
use super::layout::{push_field_attr, push_struct_lit};
use crate::generator::validate::{push_decimal_checks, push_field_rule};
use crate::generator::{
    HEADER, MAX_WIDTH, doc_name, insert_field_type, push_doc, update_field_type, use_list,
    use_paths,
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
        .then(|| table.update_columns());
    let mut output = format!(
        "{HEADER}\n//! The `{}` request and response bodies.\n\n",
        table.display_name()
    );

    let mut fields: Vec<&Column> = table.columns.iter().collect();
    fields.extend(create.iter().flatten());
    fields.extend(update.iter().flatten());
    output.push_str(&imports(
        feature,
        &fields,
        create.is_some(),
        update.is_some(),
    ));

    if feature.serves(Endpoint::List) {
        render_list_query(&mut output, feature);
    }
    if let Some(columns) = &create {
        render_create(&mut output, feature, columns);
    }
    if let Some(columns) = &update {
        render_update(&mut output, feature, columns);
    }
    render_response(&mut output, feature);
    let checked = create.iter().flatten().chain(update.iter().flatten());
    push_decimal_checks(&mut output, checked.copied());
    output
}

/// The create body's columns: the create input's, less a key the server
/// makes (`primary_key_ids`).
fn create_columns<'a>(feature: &Feature<'a>) -> Vec<&'a Column> {
    feature
        .table
        .insert_columns()
        .into_iter()
        .filter(|column| column.id_generation.is_none())
        .collect()
}

/// The `use` lines: the data layer's types the bodies convert to and from,
/// the typed ids and enums their fields hold, and the list's query.
fn imports(feature: &Feature<'_>, fields: &[&Column], create: bool, update: bool) -> String {
    let table = feature.table;
    let mut model = vec![feature.row().to_owned()];
    if create {
        model.push(format!("New{}", feature.row()));
    }
    if update {
        model.push(format!("Update{}", feature.row()));
    }
    let mut paths = Vec::new();
    for id in fields.iter().filter_map(|column| column.typed_id.as_ref()) {
        if id.module == table.module_name {
            model.push(id.name.clone());
        } else {
            paths.push(format!(
                "{}::{}::models::{}",
                feature.db_path, id.module, id.name
            ));
        }
    }
    model.sort();
    model.dedup();
    if feature.serves(Endpoint::List) {
        paths.push("crate::shared::pagination::ListQuery".to_owned());
    }
    if fields
        .iter()
        .any(|column| mentions_generated_type(&column.rust_type))
    {
        paths.push(format!("{}::types", feature.db_path));
    }
    let mut output = use_list("super::model", &model);
    output.push_str(&use_paths(&paths));
    if feature.serves(Endpoint::List) {
        output.push_str(&format!(
            "\npub use {}::listing::{};\n",
            feature.db_module(),
            feature.sort_enum()
        ));
    }
    output.push('\n');
    output
}

fn render_list_query(output: &mut String, feature: &Feature<'_>) {
    let mut doc = vec![format!(
        "`GET {}`'s query string: `?page=&per_page=&q=&sort=&order=`.",
        feature.url
    )];
    if feature.search.is_empty() {
        doc.push("`q` is ignored: the table has no search columns.".to_owned());
    } else {
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
    let target = format!("ListQuery<{}>;", feature.sort_enum());
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
    output.push_str(&format!(
        "\nimpl From<{from}> for {to} {{\n    fn from({binding}: {from}) -> Self {{\n"
    ));
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
                "`POST {}`'s body, checked by the rules of [`{new}`].",
                feature.url
            ),
            "A field for a defaulted column may be left out.",
        ],
    );
    output
        .push_str("#[derive(Debug, Clone, PartialEq, serde::Deserialize, validator::Validate)]\n");
    output.push_str(&format!("pub struct {dto} {{\n"));
    for column in columns {
        push_doc(output, 4, &[&field_doc(column)]);
        if column.is_omittable_on_insert() && column.nullable {
            push_nullable(output, feature, true);
        }
        push_field_rule(output, column);
        output.push_str(&format!(
            "    pub {}: {},\n",
            column.rust_name,
            insert_field_type(column)
        ));
    }
    output.push_str("}\n");
    let fields: Vec<String> = feature
        .table
        .insert_columns()
        .iter()
        .map(|column| match column.id_generation {
            Some(_) => format!("{}: None", column.rust_name),
            None => format!("{name}: dto.{name}", name = column.rust_name),
        })
        .collect();
    push_from(output, &dto, &new, "dto", &fields);
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
                "`PATCH {}{}`'s body, checked by the rules of [`{patch}`].",
                feature.url,
                feature.key_path()
            ),
            "Every field is optional: a field left out keeps its column's value, and",
            "`null` sets a nullable column to `NULL`.",
        ],
    );
    output.push_str(
        "#[derive(Debug, Clone, Default, PartialEq, serde::Deserialize, validator::Validate)]\n#[serde(default)]\n",
    );
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
        output.push_str(&format!(
            "    pub {}: {},\n",
            column.rust_name,
            update_field_type(column)
        ));
    }
    output.push_str("}\n");
    let fields: Vec<String> = columns
        .iter()
        .map(|column| format!("{name}: dto.{name}", name = column.rust_name))
        .collect();
    push_from(output, &dto, &patch, "dto", &fields);
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
    output.push_str(&format!("pub struct {response} {{\n"));
    for column in &feature.table.columns {
        push_doc(output, 4, &[&field_doc(column)]);
        output.push_str(&format!(
            "    pub {}: {},\n",
            column.rust_name,
            column.field_type()
        ));
    }
    output.push_str("}\n");
    let fields: Vec<String> = feature
        .table
        .columns
        .iter()
        .map(|column| format!("{name}: row.{name}", name = column.rust_name))
        .collect();
    push_from(output, row, &response, "row", &fields);
}
