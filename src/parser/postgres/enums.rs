//! Enum types (`CREATE`, `ALTER`, and `DROP TYPE`), and the value-list checks
//! that become enums.

use std::collections::HashMap;

use sqlparser::ast::{
    AlterType, AlterTypeAddValue, AlterTypeAddValuePosition, AlterTypeOperation, BinaryOperator,
    Expr, ObjectName, Value, ValueWithSpan,
};

use super::alter::drop_column;
use super::names::{ident_name, normalize_schema, object_leaf_name, object_schema_name};
use super::table::rust_type;
use crate::parser::naming;
use crate::schema::{BaseType, Column, EnumType, EnumVariant, Shape, Table};

/// Whether any column of the table has the enum type at `rust_path`, alone
/// or as an array element.
pub(super) fn uses_type(table: &Table, rust_path: &str) -> bool {
    table
        .columns
        .iter()
        .any(|column| uses_type_column(column, rust_path))
}

/// Whether the column has the enum type at `rust_path`, alone or in an array.
fn uses_type_column(column: &Column, rust_path: &str) -> bool {
    column.sqlx_type == rust_path || Shape::of(&column.sqlx_type) == Shape::List(rust_path)
}

/// Turns every value-list column into an enum named after its table and
/// column (`products.product_type` → `ProductProductType`), in table and
/// column order. A table whose value list cannot be generated (unusable or
/// colliding labels, or a name another enum already took) is reported by
/// module name with the reason, and its other value lists are not generated.
/// `earlier` holds value-list enums resolved before, whose names are taken.
pub(super) fn resolve_value_lists(
    tables: &mut [Table],
    enums: &EnumRegistry,
    earlier: &[EnumType],
) -> (Vec<EnumType>, HashMap<String, String>) {
    let mut generated: Vec<EnumType> = Vec::new();
    let mut broken = HashMap::new();
    for table in tables.iter_mut() {
        let mut table_enums = Vec::new();
        let mut failure = None;
        for column in &table.columns {
            let Some(labels) = &column.value_list else {
                continue;
            };
            let rust_name =
                naming::type_name(&format!("{}_{}", table.struct_name, column.rust_name));
            let taken = enums.has_rust_name(&rust_name)
                || earlier
                    .iter()
                    .chain(&generated)
                    .chain(&table_enums)
                    .any(|other: &EnumType| other.rust_name == rust_name);
            let variants = if taken {
                Err(format!(
                    "its Rust name `{rust_name}` is already used by another enum type"
                ))
            } else {
                enum_variants(labels.clone())
            };
            match variants {
                Ok(variants) => table_enums.push(EnumType {
                    schema: None,
                    sql_name: column.sql_name.clone(),
                    rust_name,
                    variants,
                    column: Some(format!("{}.{}", table.display_name(), column.sql_name)),
                }),
                Err(reason) => {
                    failure = Some(format!(
                        "column `{}`: value list cannot be generated: {reason}",
                        column.sql_name
                    ));
                    break;
                }
            }
        }
        if let Some(reason) = failure {
            broken.insert(table.module_name.clone(), reason);
            continue;
        }
        for enum_type in table_enums {
            let column = table
                .columns
                .iter_mut()
                .find(|column| Some(&column.sql_name) == Some(&enum_type.sql_name))
                .expect("value-list enums come from the table's columns");
            column.sqlx_type = enum_type.rust_path();
            column.rust_type = rust_type(&column.sqlx_type, column.nullable);
            generated.push(enum_type);
        }
    }
    (generated, broken)
}

/// Enum types defined so far. A type that cannot be generated (unusable or
/// colliding labels or names, or an `ALTER TYPE` that could not be applied)
/// stays registered with the reason, so a column using it fails its table.
#[derive(Default)]
pub(super) struct EnumRegistry {
    pub(super) entries: Vec<EnumEntry>,
}

pub(super) struct EnumEntry {
    pub(super) enum_type: EnumType,
    pub(super) broken: Option<String>,
}

impl EnumEntry {
    /// The column type, or why the enum cannot be generated.
    pub(super) fn rust_path(&self) -> std::result::Result<String, String> {
        match &self.broken {
            None => Ok(self.enum_type.rust_path()),
            Some(reason) => Err(format!(
                "type `{}` cannot be generated: {reason}",
                self.enum_type.display_name()
            )),
        }
    }

    /// Recomputes the variants from `labels`, breaking the enum when two
    /// labels collide or a label has no usable characters.
    fn set_labels(&mut self, labels: Vec<String>) {
        match enum_variants(labels) {
            Ok(variants) => self.enum_type.variants = variants,
            Err(reason) => self.broken = Some(reason),
        }
    }

    fn labels(&self) -> Vec<String> {
        self.enum_type
            .variants
            .iter()
            .map(|variant| variant.label.clone())
            .collect()
    }
}

/// An `ALTER TYPE … RENAME TO`: the columns of the enum type at `from` now
/// have the type at `to`.
pub(super) struct RenamedEnum {
    from: String,
    to: String,
}

/// An `ALTER TYPE` that broke an enum: every table using it fails.
pub(super) struct BrokenEnum {
    pub(super) rust_path: String,
    pub(super) reason: String,
}

impl EnumRegistry {
    fn position(&self, name: &ObjectName) -> Option<usize> {
        let schema = normalize_schema(object_schema_name(name));
        let sql_name = object_leaf_name(name);
        self.entries.iter().position(|entry| {
            entry.enum_type.sql_name == sql_name && entry.enum_type.schema == schema
        })
    }

    /// Whether any enum type, generatable or not, has this Rust name.
    fn has_rust_name(&self, rust_name: &str) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.enum_type.rust_name == rust_name)
    }

    pub(super) fn lookup(&self, name: &ObjectName) -> Option<&EnumEntry> {
        self.position(name).map(|index| &self.entries[index])
    }

    /// The Rust name of the enum type `schema.sql_name`: the type name in
    /// UpperCamelCase, prefixed with the schema when another enum (other
    /// than the one at `except`) already took that name. Also returns why
    /// the name cannot be generated.
    fn rust_name_for(
        &self,
        schema: Option<&str>,
        sql_name: &str,
        except: Option<usize>,
    ) -> (String, Option<String>) {
        let taken = |rust_name: &str| {
            self.entries.iter().enumerate().any(|(index, entry)| {
                Some(index) != except && entry.enum_type.rust_name == rust_name
            })
        };
        let mut rust_name = naming::type_name(sql_name);
        if taken(&rust_name)
            && let Some(schema) = schema
        {
            rust_name = naming::type_name(&format!("{schema}_{sql_name}"));
        }
        let broken = if rust_name.is_empty() {
            Some(format!(
                "`{sql_name}` does not contain any characters usable in a Rust identifier"
            ))
        } else if taken(&rust_name) {
            Some(format!(
                "its Rust name `{rust_name}` is already used by another enum type"
            ))
        } else {
            None
        };
        (rust_name, broken)
    }

    /// Registers a `CREATE TYPE … AS ENUM`, named by [`Self::rust_name_for`].
    pub(super) fn define(&mut self, name: &ObjectName, labels: impl Iterator<Item = String>) {
        if let Some(index) = self.position(name) {
            self.entries[index].broken = Some("the type is defined more than once".to_owned());
            return;
        }
        let schema = normalize_schema(object_schema_name(name));
        let sql_name = object_leaf_name(name);
        let (rust_name, broken) = self.rust_name_for(schema.as_deref(), &sql_name, None);
        let mut entry = EnumEntry {
            enum_type: EnumType {
                schema,
                sql_name,
                rust_name,
                variants: Vec::new(),
                column: None,
            },
            broken,
        };
        entry.set_labels(labels.collect());
        self.entries.push(entry);
    }

    /// Applies an `ALTER TYPE` to a known enum. A `RENAME TO` returns the
    /// enum's old and new paths, for the columns using it to follow. An
    /// alteration that breaks a generatable enum returns its path and the
    /// reason. Unknown and already broken types are left alone.
    pub(super) fn alter(
        &mut self,
        alter: &AlterType,
    ) -> std::result::Result<Option<RenamedEnum>, BrokenEnum> {
        let Some(index) = self.position(&alter.name) else {
            return Ok(None);
        };
        if self.entries[index].broken.is_some() {
            return Ok(None);
        }
        let mut labels = self.entries[index].labels();
        let mut renamed = None;
        let applied = match &alter.operation {
            AlterTypeOperation::AddValue(add) => add_enum_value(&mut labels, add),
            AlterTypeOperation::RenameValue(rename) => {
                let (from, to) = (&rename.from.value, &rename.to.value);
                match labels.iter().position(|label| label == from) {
                    _ if labels.contains(to) => {
                        Err(format!("RENAME VALUE: label `{to}` already exists"))
                    }
                    Some(index) => {
                        labels[index] = to.clone();
                        Ok(())
                    }
                    None => Err(format!("RENAME VALUE: label `{from}` does not exist")),
                }
            }
            AlterTypeOperation::Rename(rename) => {
                let sql_name = ident_name(&rename.new_name);
                let schema = self.entries[index].enum_type.schema.clone();
                let exists = self.entries.iter().any(|entry| {
                    entry.enum_type.sql_name == sql_name && entry.enum_type.schema == schema
                });
                let (rust_name, broken) =
                    self.rust_name_for(schema.as_deref(), &sql_name, Some(index));
                match (exists, broken) {
                    (true, _) => {
                        let name = match &schema {
                            Some(schema) => format!("{schema}.{sql_name}"),
                            None => sql_name,
                        };
                        Err(format!("RENAME TO: type `{name}` already exists"))
                    }
                    (false, Some(reason)) => Err(format!("RENAME TO: {reason}")),
                    (false, None) => {
                        let enum_type = &mut self.entries[index].enum_type;
                        let from = enum_type.rust_path();
                        enum_type.sql_name = sql_name;
                        enum_type.rust_name = rust_name;
                        renamed = Some(RenamedEnum {
                            from,
                            to: enum_type.rust_path(),
                        });
                        Ok(())
                    }
                }
            }
        };
        let entry = &mut self.entries[index];
        let reason = match applied {
            Ok(()) => {
                entry.set_labels(labels);
                match entry.broken.clone() {
                    None => return Ok(renamed),
                    Some(reason) => reason,
                }
            }
            Err(reason) => reason,
        };
        entry.broken = Some(reason.clone());
        Err(BrokenEnum {
            rust_path: entry.enum_type.rust_path(),
            reason: format!("ALTER TYPE `{}`: {reason}", entry.enum_type.display_name()),
        })
    }

    /// Applies a `DROP TYPE` to one name: a known enum is forgotten, and its
    /// column type returned so the tables using it can follow.
    pub(super) fn drop(&mut self, name: &ObjectName) -> Option<String> {
        let index = self.position(name)?;
        Some(self.entries.remove(index).enum_type.rust_path())
    }

    /// The enums the `types` module generates.
    pub(super) fn into_generatable(self) -> Vec<EnumType> {
        self.entries
            .into_iter()
            .filter(|entry| entry.broken.is_none())
            .map(|entry| entry.enum_type)
            .collect()
    }
}

fn add_enum_value(
    labels: &mut Vec<String>,
    add: &AlterTypeAddValue,
) -> std::result::Result<(), String> {
    let value = &add.value.value;
    if labels.contains(value) {
        if add.if_not_exists {
            return Ok(());
        }
        return Err(format!("ADD VALUE: label `{value}` already exists"));
    }
    let index = match &add.position {
        None => labels.len(),
        Some(
            AlterTypeAddValuePosition::Before(neighbor)
            | AlterTypeAddValuePosition::After(neighbor),
        ) => {
            let found = labels
                .iter()
                .position(|label| *label == neighbor.value)
                .ok_or_else(|| format!("ADD VALUE: label `{}` does not exist", neighbor.value))?;
            match &add.position {
                Some(AlterTypeAddValuePosition::After(_)) => found + 1,
                _ => found,
            }
        }
    };
    labels.insert(index, value.clone());
    Ok(())
}

/// One variant per label, in label order, each named by its label in
/// UpperCamelCase (`Very High` → `VeryHigh`). An enum with no labels, a
/// label with no usable characters, or two labels naming the same variant
/// cannot be generated.
fn enum_variants(labels: Vec<String>) -> std::result::Result<Vec<EnumVariant>, String> {
    if labels.is_empty() {
        return Err("the enum has no labels".to_owned());
    }
    let mut variants: Vec<EnumVariant> = Vec::with_capacity(labels.len());
    for label in labels {
        let rust_name = naming::variant_name(&label).ok_or_else(|| {
            format!("label `{label}` has no characters usable in a Rust identifier")
        })?;
        if let Some(other) = variants
            .iter()
            .find(|variant| variant.rust_name == rust_name)
        {
            return Err(format!(
                "labels `{}` and `{label}` both map to the variant `{rust_name}`",
                other.label
            ));
        }
        variants.push(EnumVariant { label, rust_name });
    }
    Ok(variants)
}

/// Moves every column of a renamed enum type (or an array of it) to the
/// enum's new path.
pub(super) fn retype_columns(table: &mut Table, renamed: &RenamedEnum) {
    for column in &mut table.columns {
        if column.sqlx_type == renamed.from {
            column.sqlx_type.clone_from(&renamed.to);
        } else if Shape::of(&column.sqlx_type) == Shape::List(&renamed.from) {
            column.sqlx_type = format!("Vec<{}>", renamed.to);
        } else {
            continue;
        }
        column.rust_type = rust_type(&column.sqlx_type, column.nullable);
    }
}

/// Drops every column of the enum type at `rust_path` (or an array of it),
/// as `DROP TYPE … CASCADE` does.
pub(super) fn drop_columns_of_type(table: &mut Table, rust_path: &str) {
    let doomed: Vec<String> = table
        .columns
        .iter()
        .filter(|column| uses_type_column(column, rust_path))
        .map(|column| column.sql_name.clone())
        .collect();
    for name in doomed {
        // The column exists, so dropping it cannot fail.
        let _ = drop_column(table, &name, true);
    }
}

/// `expr` written as the `col IN ('a', 'b', …)` [`value_list_check`]
/// reads, when it is the form `pg_dump` writes a value list in:
/// `col = ANY (ARRAY['a'::text, 'b'::text])`, with casts on the column, the
/// array or its values (`(col)::text = ANY ((ARRAY['a'::character
/// varying])::text[])` on a `varchar`). Any other shape is `None`.
///
/// `ALTER TABLE … ADD CONSTRAINT … CHECK` reads this form; `CREATE TABLE`
/// doesn't, so the code generated from existing schemas doesn't change.
pub(super) fn any_array_as_in_list(expr: &Expr) -> Option<Expr> {
    fn peeled(expr: &Expr) -> &Expr {
        match expr {
            Expr::Nested(inner) | Expr::Cast { expr: inner, .. } => peeled(inner),
            other => other,
        }
    }
    let Expr::AnyOp {
        left,
        compare_op: BinaryOperator::Eq,
        right,
        ..
    } = peeled(expr)
    else {
        return None;
    };
    let (Expr::Identifier(column), Expr::Array(array)) = (peeled(left), peeled(right)) else {
        return None;
    };
    let list = array
        .elem
        .iter()
        .map(|item| match peeled(item) {
            value @ Expr::Value(ValueWithSpan {
                value: Value::SingleQuotedString(_),
                ..
            }) => Some(value.clone()),
            _ => None,
        })
        .collect::<Option<Vec<Expr>>>()?;
    Some(Expr::InList {
        expr: Box::new(Expr::Identifier(column.clone())),
        list,
        negated: false,
    })
}

/// The column and values of a value-list check, `col IN ('a', 'b', …)`
/// (parenthesized or not), with duplicate values dropped. Any other shape,
/// including `NOT IN` and lists holding anything but string literals, is
/// `None`.
pub(super) fn value_list_check(expr: &Expr) -> Option<(String, Vec<String>)> {
    match expr {
        Expr::Nested(inner) => value_list_check(inner),
        Expr::InList {
            expr,
            list,
            negated: false,
        } => {
            let Expr::Identifier(column) = expr.as_ref() else {
                return None;
            };
            let mut labels: Vec<String> = Vec::with_capacity(list.len());
            for item in list {
                let Expr::Value(value) = item else {
                    return None;
                };
                let Value::SingleQuotedString(label) = &value.value else {
                    return None;
                };
                if !labels.contains(label) {
                    labels.push(label.clone());
                }
            }
            Some((ident_name(column), labels))
        }
        _ => None,
    }
}

/// Restricts a text column to a list of values. A second value list on the
/// same column keeps only the values both allow. Other column types are left
/// alone, like checks of any other shape.
pub(super) fn restrict_to_values(column: &mut Column, labels: Vec<String>) {
    if column.base_type() != BaseType::Text {
        return;
    }
    column.value_list = Some(match column.value_list.take() {
        Some(existing) => existing
            .into_iter()
            .filter(|label| labels.contains(label))
            .collect(),
        None => labels,
    });
}
