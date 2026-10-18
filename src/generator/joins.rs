//! Joined reads (ADR 0014): for each to-one relationship of a table with a
//! primary key, `get_with_<relationship>` reads a row and its related row
//! in one statement, returning a `{Row}With{Relationship}` struct.
//!
//! The rows are joined under fixed aliases (`t` for the table read, `r` for
//! the related table) and every column is selected under an alias naming
//! its side (`t_id`, `r_id`), so columns the two tables share decode
//! unambiguously. The table read is scoped in `WHERE`; the related table's
//! tenant and soft-delete conditions go in the join's `ON` clause, so a
//! hidden related row reads as absent instead of hiding the row.

use heck::{ToSnakeCase, ToUpperCamelCase};

use crate::schema::{Column, Relationship, RelationshipKind, Table};

use super::relationships;
use super::sql::{ident, live_condition, str_literal, table_ref, tenant_condition};
use super::{
    MAX_WIDTH, StaticQuery, doc_name, keys, param_name, param_type, push_derives, push_doc,
    push_fn_head, serde_derives,
};

/// The alias of the table read.
const ROW_ALIAS: &str = "t";
/// The alias of the related table.
const RELATED_ALIAS: &str = "r";

/// One to-one relationship's joined read.
pub struct Join<'a> {
    table: &'a Table,
    relationship: &'a Relationship,
    target: &'a Table,
    /// The joining column on `table`'s rows.
    column: &'a Column,
    /// The joining column on `target`'s rows.
    target_column: &'a Column,
}

/// The joined reads of `table`: one per to-one relationship, none when the
/// table has no primary key to read a row by.
pub fn joins<'a>(table: &'a Table, tables: &'a [Table]) -> Vec<Join<'a>> {
    if table.primary_key.is_empty() {
        return Vec::new();
    }
    table
        .relationships
        .iter()
        .filter(|relationship| !relationship.kind.is_to_many())
        .map(|relationship| {
            let resolved = relationships::resolve(table, relationship, tables);
            Join {
                table,
                relationship,
                target: resolved.target,
                column: resolved.column,
                target_column: resolved.target_column,
            }
        })
        .collect()
}

/// The `use` lines the joined reads of `table` need beyond the row types:
/// `PgRow` and the `Row` trait, plus `ValueRef` when a related row is
/// optional. Empty without joined reads. Ordered the way rustfmt orders
/// them: `Row` before `postgres`, and a braced list last.
pub fn sqlx_imports(joins: &[Join<'_>]) -> String {
    if joins.is_empty() {
        String::new()
    } else if joins.iter().any(Join::optional) {
        "use sqlx::postgres::PgRow;\nuse sqlx::{Row, ValueRef};\n".to_owned()
    } else {
        "use sqlx::Row;\nuse sqlx::postgres::PgRow;\n".to_owned()
    }
}

/// The columns whose value types the joined reads' parameters name: the
/// table's tenant (or else the related table's) and single key column.
pub fn param_columns<'a>(joins: &[Join<'a>]) -> Vec<&'a Column> {
    let mut columns = Vec::new();
    for join in joins {
        columns.extend(join.tenant());
        let keys = join.table.key_columns();
        if keys.len() == 1 {
            columns.extend(keys);
        }
    }
    columns
}

/// The key struct `table`'s joined reads take, when its key is composite.
pub fn key_struct(table: &Table, joins: &[Join<'_>]) -> Option<String> {
    if joins.is_empty() || table.key_columns().len() < 2 {
        return None;
    }
    Some(keys::key_type(table))
}

impl<'a> Join<'a> {
    /// The relationship's name.
    pub fn name(&self) -> &str {
        &self.relationship.name
    }

    /// Whether the related row may be absent: a has-one, or a belongs-to
    /// whose foreign key is nullable or whose related table hides rows (by
    /// tenant or soft delete). Otherwise the foreign key guarantees it.
    pub fn optional(&self) -> bool {
        self.relationship.kind == RelationshipKind::HasOne
            || self.column.nullable
            || self.target.tenant().is_some()
            || self.target.soft_delete_column.is_some()
    }

    /// The tenant parameter: the table's tenant column, or the related
    /// table's when only it has one. Both tables' conditions bind it as `$1`.
    pub fn tenant(&self) -> Option<&'a Column> {
        self.table.tenant().or_else(|| self.target.tenant())
    }

    pub fn struct_name(&self) -> String {
        format!(
            "{}With{}",
            self.table.struct_name,
            self.relationship.name.to_upper_camel_case()
        )
    }

    /// The field holding the row read: the row's name (`product`), or `row`
    /// when the relationship has that name.
    pub fn row_field(&self) -> String {
        let name = self.table.struct_name.to_snake_case();
        if name == self.relationship.name {
            "row".to_owned()
        } else {
            name
        }
    }

    pub fn function_name(&self) -> String {
        format!("get_with_{}", self.relationship.name)
    }

    fn related_type(&self) -> String {
        if self.optional() {
            format!("Option<{}>", self.target.struct_name)
        } else {
            self.target.struct_name.clone()
        }
    }

    /// The related row, described for a doc comment.
    fn related_doc(&self) -> String {
        let target = doc_name(&self.target.display_name());
        match self.relationship.kind {
            RelationshipKind::BelongsTo => format!(
                "the {target} row its {} references",
                doc_name(&self.column.sql_name)
            ),
            RelationshipKind::HasOne | RelationshipKind::HasMany | RelationshipKind::ManyToMany => {
                format!(
                    "the {target} row whose {} references it",
                    doc_name(&self.target_column.sql_name)
                )
            }
        }
    }

    /// Renders the joined struct (with serde derives when `serde`), its
    /// `FromRow`, and `get_with_<relationship>`.
    pub fn render(&self, output: &mut String, serde: bool) {
        self.render_struct(output, serde);
        self.render_from_row(output);
        self.render_function(output);
    }

    fn render_struct(&self, output: &mut String, serde: bool) {
        let name = format!("`{}`", self.relationship.name);
        output.push('\n');
        push_doc(
            output,
            0,
            &[
                &format!(
                    "A {} row with its {name}, as [`{}`] reads it.",
                    doc_name(&self.table.display_name()),
                    self.function_name()
                ),
                "",
                &format!("The {name} is {}.", self.related_doc()),
            ],
        );
        output.push_str("#[derive(Debug, Clone, PartialEq)]\n");
        push_derives(output, serde_derives(serde));
        output.push_str(&format!("pub struct {} {{\n", self.struct_name()));
        push_doc(output, 4, &["The row read."]);
        output.push_str(&format!(
            "    pub {}: {},\n",
            self.row_field(),
            self.table.struct_name
        ));
        let related = if self.optional() {
            format!("Its {name}, or `None` when there is none.")
        } else {
            format!("Its {name}.")
        };
        push_doc(output, 4, &[&related]);
        output.push_str(&format!(
            "    pub {}: {},\n",
            self.relationship.name,
            self.related_type()
        ));
        output.push_str("}\n");
    }

    /// Decodes both rows from their aliased columns. An optional related row
    /// is absent when its joining column is `NULL`, which a joined row's
    /// never is.
    fn render_from_row(&self, output: &mut String) {
        output.push_str(&format!(
            "\nimpl<'r> sqlx::FromRow<'r, PgRow> for {} {{\n",
            self.struct_name()
        ));
        output.push_str("    fn from_row(row: &'r PgRow) -> Result<Self, sqlx::Error> {\n");
        if self.optional() {
            output.push_str(&format!(
                "        let related = !row.try_get_raw({})?.is_null();\n",
                str_literal(&alias(RELATED_ALIAS, self.target, self.target_column))
            ));
        }
        output.push_str("        Ok(Self {\n");
        output.push_str(&format!(
            "            {}: {} {{\n",
            self.row_field(),
            self.table.struct_name
        ));
        push_fields(output, 16, self.table, ROW_ALIAS);
        output.push_str("            },\n");
        let target = &self.target.struct_name;
        if self.optional() {
            output.push_str(&format!(
                "            {}: if related {{\n",
                self.relationship.name
            ));
            output.push_str(&format!("                Some({target} {{\n"));
            push_fields(output, 20, self.target, RELATED_ALIAS);
            output.push_str(
                "                })\n            } else {\n                None\n            },\n",
            );
        } else {
            output.push_str(&format!(
                "            {}: {target} {{\n",
                self.relationship.name
            ));
            push_fields(output, 16, self.target, RELATED_ALIAS);
            output.push_str("            },\n");
        }
        output.push_str("        })\n    }\n}\n");
    }

    fn render_function(&self, output: &mut String) {
        let table = self.table;
        let target = self.target;
        let tenant = self.tenant();
        let first_key = 1 + usize::from(tenant.is_some());

        let mut sql = vec!["SELECT".to_owned()];
        let selected: Vec<String> = table
            .columns
            .iter()
            .map(|column| (ROW_ALIAS, table, column))
            .chain(
                target
                    .columns
                    .iter()
                    .map(|column| (RELATED_ALIAS, target, column)),
            )
            .map(|(side, owner, column)| {
                format!(
                    "{side}.{} AS {}",
                    ident(&column.sql_name),
                    ident(&alias(side, owner, column))
                )
            })
            .collect();
        for (index, column) in selected.iter().enumerate() {
            let separator = if index + 1 < selected.len() { "," } else { "" };
            sql.push(format!("    {column}{separator}"));
        }
        sql.push(format!("FROM {} AS {ROW_ALIAS}", table_ref(table)));
        let join = if self.optional() { "LEFT JOIN" } else { "JOIN" };
        sql.push(format!("{join} {} AS {RELATED_ALIAS}", table_ref(target)));
        sql.push(format!(
            "    ON {RELATED_ALIAS}.{} = {ROW_ALIAS}.{}",
            ident(&self.target_column.sql_name),
            ident(&self.column.sql_name)
        ));
        for condition in tenant_condition(target, Some(RELATED_ALIAS))
            .into_iter()
            .chain(live_condition(target, Some(RELATED_ALIAS)))
        {
            sql.push(format!("    AND {condition}"));
        }
        let conditions: Vec<String> = tenant_condition(table, Some(ROW_ALIAS))
            .into_iter()
            .chain(
                table
                    .key_columns()
                    .iter()
                    .enumerate()
                    .map(|(index, column)| {
                        format!(
                            "{ROW_ALIAS}.{} = ${}",
                            ident(&column.sql_name),
                            first_key + index
                        )
                    }),
            )
            .chain(live_condition(table, Some(ROW_ALIAS)))
            .collect();
        sql.push(format!("WHERE {}", conditions.join(" AND ")));

        let name = format!("`{}`", self.relationship.name);
        let mut doc = vec![
            format!("Fetches the row with the given primary key and its {name},"),
            "in one query, or `None` if there is no such row.".to_owned(),
        ];
        if table.soft_delete_column.is_some() {
            doc.push("A soft-deleted row is `None`.".to_owned());
        }
        if self.optional() {
            doc.push(format!("The {name} is `None` when there is none."));
        }
        if target.soft_delete_column.is_some() {
            doc.push(format!("A soft-deleted {name} reads as none."));
        }
        if target.tenant().is_some() {
            doc.push(format!("Another tenant's {name} reads as none."));
        }
        if tenant.is_some() {
            doc.push("Only the given tenant's rows are read.".to_owned());
        }
        output.push('\n');
        let doc: Vec<&str> = doc.iter().map(String::as_str).collect();
        push_doc(output, 0, &doc);

        let mut params: Vec<String> = tenant
            .map(|column| format!("{}: {}", param_name(column), param_type(column)))
            .into_iter()
            .collect();
        if !table.key_columns().is_empty() {
            params.push(keys::key_param(table));
        }
        let joined = self.struct_name();
        push_fn_head(
            output,
            &self.function_name(),
            &params,
            &format!("Result<Option<{joined}>, sqlx::Error>"),
        );
        StaticQuery {
            call: &format!("sqlx::query_as::<_, {joined}>"),
            sql,
            binds: tenant
                .map(param_name)
                .into_iter()
                .chain(keys::key_binds(table))
                .collect(),
            execute: "fetch_optional(executor)",
        }
        .push(output, 4, "", "");
        output.push_str("}\n");
    }
}

/// PostgreSQL's longest identifier, in bytes (`NAMEDATALEN - 1`). It cuts a
/// longer alias to this length, which can make two aliases one.
const MAX_ALIAS_BYTES: usize = 63;

/// The alias `table`'s column is selected under: its side's alias, `_`, and
/// its name (`t_id`). Within a side, column names are unique, and the two
/// sides' prefixes differ. An alias too long for PostgreSQL also names the
/// column's position (`t3_<name>`, cut to 63 bytes): no `<side>_` alias
/// starts that way, and positions differ within a side.
fn alias(side: &str, table: &Table, column: &Column) -> String {
    let alias = format!("{side}_{}", column.sql_name);
    if alias.len() <= MAX_ALIAS_BYTES {
        return alias;
    }
    let position = table
        .columns
        .iter()
        .position(|other| other.sql_name == column.sql_name)
        .unwrap_or_default();
    let mut alias = format!("{side}{position}_{}", column.sql_name);
    let mut len = MAX_ALIAS_BYTES;
    while !alias.is_char_boundary(len) {
        len -= 1;
    }
    alias.truncate(len);
    alias
}

/// `field: row.try_get("<alias>")?,` for every column of `table`, with the
/// call on its own line when the line is too long, as rustfmt breaks it.
fn push_fields(output: &mut String, indent: usize, table: &Table, side: &str) {
    let pad = " ".repeat(indent);
    for column in &table.columns {
        let name = &column.rust_name;
        let literal = str_literal(&alias(side, table, column));
        let call = format!("try_get({literal})?,");
        let one_line = format!("{pad}{name}: row.{call}");
        let below = format!("{pad}    .{call}");
        // rustfmt keeps a line ending in `?,` one column short of the width.
        if one_line.len() < MAX_WIDTH - 1 {
            output.push_str(&one_line);
            output.push('\n');
        } else if below.len() < MAX_WIDTH - 1 {
            output.push_str(&format!("{pad}{name}: row\n{below}\n"));
        } else {
            output.push_str(&format!(
                "{pad}{name}: row.try_get(\n{pad}    {literal},\n{pad})?,\n"
            ));
        }
    }
}
