use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use sqlparser::ast::{
    AlterColumnOperation, AlterTableOperation, AlterType, AlterTypeAddValue,
    AlterTypeAddValuePosition, AlterTypeOperation, ArrayElemTypeDef, ColumnDef, ColumnOption,
    CreateIndex, CreateTable, CreateTrigger, DataType, DropTrigger, ExactNumberInfo, Expr, Ident,
    IndexColumn, ObjectName, ObjectNamePart, ObjectType, Spanned, Statement, TableConstraint,
    TimezoneInfo, TriggerEvent, TriggerObject, TriggerPeriod, UserDefinedTypeRepresentation, Value,
};
use sqlparser::dialect::PostgreSqlDialect;
use sqlparser::parser::Parser;

use super::diagnostics::TableFailure;
use super::naming;
use crate::report::display_path;
use crate::schema::rust_type;
use crate::schema::{
    BaseType, Column, EnumType, EnumVariant, ForeignKey, KeyConstraint, Schema, Shape, Table,
    UniqueKey,
};

pub struct ParsedSchema {
    pub schema: Schema,
    pub failures: Vec<TableFailure>,
    /// Where each table in `schema` was created (its `CREATE TABLE`), by
    /// module name, so a later check can fail it at that location.
    created: HashMap<String, (PathBuf, Option<usize>)>,
    /// The `[modules]` keys that named no `CREATE TABLE` of the schema.
    pub unmatched_modules: Vec<String>,
}

impl ParsedSchema {
    /// Fails every table `reason` gives a reason for: the table leaves
    /// `schema.tables` and joins `failures`, located at its `CREATE TABLE`.
    pub fn fail_tables(&mut self, mut reason: impl FnMut(&Table) -> Option<String>) {
        let mut failed = Vec::new();
        self.schema.tables.retain(|table| {
            let Some(reason) = reason(table) else {
                return true;
            };
            failed.push((table.clone(), reason));
            false
        });
        for (table, reason) in failed {
            let (path, line) = self.created.remove(&table.module_name).unwrap_or_default();
            self.failures.push(TableFailure {
                path: Some(path).filter(|path| !path.as_os_str().is_empty()),
                line,
                schema: table.schema.clone(),
                ..table_failure(&table.sql_name, reason)
            });
        }
    }
}

/// Root modules rok-sqlc always generates next to the resource modules.
const GENERATED_MODULES: &[&str] = &["nullable", "schema", "search", "session", "types"];

/// One schema input: a migration file's path and its SQL.
pub struct SchemaSource<'a> {
    pub path: &'a Path,
    pub sql: &'a str,
}

/// Parses one schema text. Failures carry the line of the statement that
/// failed but no path.
pub fn parse_schema(sql: &str) -> Result<ParsedSchema> {
    let mut parsed = parse_sources(&[SchemaSource {
        path: Path::new(""),
        sql,
    }])?;
    for failure in &mut parsed.failures {
        failure.path = None;
    }
    Ok(parsed)
}

/// Parses schema inputs in order, as one schema. Each file is parsed on its
/// own, so every failure names its file and the line, within that file, of
/// the statement that failed: a table's `CREATE TABLE`, or the `ALTER TABLE`
/// it could not apply.
///
/// Later statements change earlier tables: `ALTER TABLE` adds, drops, and
/// renames columns and adds primary key, unique, foreign key, and value-list
/// check constraints, and `CREATE UNIQUE INDEX` on plain columns records a unique
/// key. Any other `ALTER TABLE` clause on a known table fails that table.
/// `CREATE TRIGGER` and `DROP TRIGGER` keep track of each table's row-level
/// `BEFORE INSERT` triggers, by name; the trigger functions are not read.
/// `DROP TABLE` removes a table, together with the foreign keys other tables
/// hold to it (as `CASCADE` would), and `DROP TYPE` removes an enum type.
/// Statements that leave the model unchanged (row-level security, policies,
/// non-unique indexes, grants, other triggers, comments, functions, `SELECT`
/// calls) and statements on unknown tables are ignored on purpose.
pub fn parse_sources(sources: &[SchemaSource<'_>]) -> Result<ParsedSchema> {
    parse_sources_with_modules(sources, &BTreeMap::new())
}

/// [`parse_sources`] with module renames (the `[modules]` configuration):
/// a table an entry names (see [`names_table`]) takes the entry's module
/// name, and its struct name follows. Entries naming no `CREATE TABLE` are
/// listed in [`ParsedSchema::unmatched_modules`].
pub fn parse_sources_with_modules(
    sources: &[SchemaSource<'_>],
    modules: &BTreeMap<String, String>,
) -> Result<ParsedSchema> {
    let mut unmatched_modules: BTreeSet<&String> = modules.keys().collect();
    let mut rename = |mut table: Table| {
        for (key, module) in modules {
            if names_table(key, table.schema.as_deref(), &table.sql_name) {
                unmatched_modules.remove(key);
                table.struct_name = naming::struct_name(module);
                table.module_name.clone_from(module);
            }
        }
        table
    };
    let dialect = PostgreSqlDialect {};
    let mut tables: Vec<Table> = Vec::new();
    let mut failures = Vec::new();
    let mut seen_modules: HashSet<String> = HashSet::new();
    let mut enums = EnumRegistry::default();
    let mut others = OtherRelations::default();
    // Where each table was created, for failures found after every statement.
    let mut created: HashMap<String, (PathBuf, Option<usize>)> = HashMap::new();

    for source in sources {
        let statements = Parser::parse_sql(&dialect, source.sql).with_context(|| {
            if source.path.as_os_str().is_empty() {
                "invalid PostgreSQL SQL syntax".to_owned()
            } else {
                format!(
                    "invalid PostgreSQL SQL syntax in `{}`",
                    display_path(source.path)
                )
            }
        })?;

        for statement in statements {
            let line = usize::try_from(statement.span().start.line).ok();
            let failure = match statement {
                Statement::Drop {
                    object_type: ObjectType::Table,
                    names,
                    ..
                } => {
                    for name in &names {
                        drop_table(&mut tables, &mut failures, name, |table| {
                            seen_modules.remove(&table.module_name);
                            created.remove(&table.module_name);
                        });
                    }
                    continue;
                }
                Statement::Drop {
                    object_type: ObjectType::Type,
                    names,
                    cascade,
                    ..
                } => {
                    for name in &names {
                        let Some(rust_path) = enums.drop(name) else {
                            continue;
                        };
                        // sqlparser gives `DROP` no span of its own; each name has one.
                        let line = usize::try_from(name.span().start.line).ok();
                        let (failed, kept) = std::mem::take(&mut tables)
                            .into_iter()
                            .partition(|table| !cascade && uses_type(table, &rust_path));
                        tables = kept;
                        for table in &mut tables {
                            drop_columns_of_type(table, &rust_path);
                        }
                        for table in failed {
                            failures.push(TableFailure {
                                path: Some(source.path.to_path_buf()),
                                line,
                                schema: table.schema.clone(),
                                ..table_failure(
                                    &table.sql_name,
                                    format!(
                                        "DROP TYPE `{}`: a column still uses the type; drop the column first or use CASCADE",
                                        object_leaf_name(name)
                                    ),
                                )
                            });
                        }
                    }
                    continue;
                }
                Statement::CreateTable(create) => {
                    let schema = object_schema_name(&create.name);
                    let mut taken = relation_names(&tables, &others, schema.as_deref());
                    // PostgreSQL makes a serial column's sequence before the
                    // table's constraints, so their names avoid it.
                    others.create_sequences(&create.name, &create.columns, &mut taken);
                    match build_table(&create, &enums, &taken).map(&mut rename) {
                        Ok(table) if GENERATED_MODULES.contains(&table.module_name.as_str()) => {
                            table_failure(
                                &table.sql_name,
                                format!(
                                    "module name `{}` is reserved for the generated `db/{}.rs`",
                                    table.module_name, table.module_name
                                ),
                            )
                        }
                        Ok(table) if seen_modules.insert(table.module_name.clone()) => {
                            created.insert(
                                table.module_name.clone(),
                                (source.path.to_path_buf(), line),
                            );
                            tables.push(table);
                            continue;
                        }
                        Ok(table) => table_failure(
                            &table.sql_name,
                            format!(
                                "module name `{}` is already used by another table; rename one under `[modules]`",
                                table.module_name
                            ),
                        ),
                        Err(failure) => TableFailure {
                            schema: object_schema_name(&create.name),
                            ..failure
                        },
                    }
                }
                Statement::AlterTable {
                    name, operations, ..
                } => {
                    let Some(index) = find_table(&tables, &name) else {
                        continue;
                    };
                    let mut taken =
                        relation_names(&tables, &others, tables[index].schema.as_deref());
                    let added = operations.iter().filter_map(|operation| match operation {
                        AlterTableOperation::AddColumn { column_def, .. } => Some(column_def),
                        _ => None,
                    });
                    others.create_sequences(&name, added, &mut taken);
                    match apply_alter(&mut tables[index], &operations, &enums, &taken) {
                        Ok(()) => continue,
                        Err(reason) => {
                            let table = tables.remove(index);
                            TableFailure {
                                schema: table.schema.clone(),
                                ..table_failure(&table.sql_name, reason)
                            }
                        }
                    }
                }
                Statement::CreateIndex(index) => {
                    let schema = object_schema_name(&index.table_name);
                    let taken = relation_names(&tables, &others, schema.as_deref());
                    others.create_index(&index, &taken);
                    record_unique_index(&mut tables, &index);
                    continue;
                }
                Statement::CreateView { name, .. } | Statement::CreateSequence { name, .. } => {
                    others.create(&name);
                    continue;
                }
                Statement::Drop {
                    object_type:
                        ObjectType::Index
                        | ObjectType::View
                        | ObjectType::MaterializedView
                        | ObjectType::Sequence,
                    names,
                    ..
                } => {
                    for name in &names {
                        others.drop(name);
                    }
                    continue;
                }
                Statement::CreateTrigger(trigger) => {
                    record_trigger(&mut tables, &trigger);
                    continue;
                }
                Statement::DropTrigger(trigger) => {
                    forget_trigger(&mut tables, &trigger);
                    continue;
                }
                Statement::CreateType {
                    name,
                    representation: UserDefinedTypeRepresentation::Enum { labels },
                } => {
                    enums.define(&name, labels.iter().map(|label| label.value.clone()));
                    continue;
                }
                Statement::AlterType(alter) => {
                    // sqlparser gives `ALTER TYPE` no span of its own; its name has one.
                    let line = usize::try_from(alter.name.span().start.line).ok();
                    let reason = match enums.alter(&alter) {
                        Ok(None) => continue,
                        Ok(Some(renamed)) => {
                            for table in &mut tables {
                                retype_columns(table, &renamed);
                            }
                            continue;
                        }
                        Err(reason) => reason,
                    };
                    // Tables already using the type cannot be generated against it.
                    let (failed, kept) = std::mem::take(&mut tables)
                        .into_iter()
                        .partition(|table| uses_type(table, &reason.rust_path));
                    tables = kept;
                    for table in failed {
                        failures.push(TableFailure {
                            path: Some(source.path.to_path_buf()),
                            line,
                            schema: table.schema.clone(),
                            ..table_failure(&table.sql_name, reason.reason.clone())
                        });
                    }
                    continue;
                }
                _ => continue,
            };
            failures.push(TableFailure {
                path: Some(source.path.to_path_buf()),
                line,
                ..failure
            });
        }
    }

    // Value lists become enums once every statement has shaped their tables.
    let (value_lists, broken) = resolve_value_lists(&mut tables, &enums);
    tables.retain(|table| {
        let Some(reason) = broken.get(&table.module_name) else {
            return true;
        };
        let (path, line) = created.remove(&table.module_name).unwrap_or_default();
        failures.push(TableFailure {
            path: Some(path),
            line,
            ..table_failure(&table.sql_name, reason.clone())
        });
        false
    });

    if tables.is_empty() && failures.is_empty() {
        bail!("no supported CREATE TABLE statements found");
    }

    let mut all_enums = enums.into_generatable();
    all_enums.extend(value_lists);
    Ok(ParsedSchema {
        schema: Schema {
            tables,
            enums: all_enums,
        },
        failures,
        created,
        unmatched_modules: unmatched_modules.into_iter().cloned().collect(),
    })
}

/// Whether a `[modules]` key names the table with this schema and SQL name:
/// the key is its schema-qualified name or its SQL name, and `public.` and
/// no schema agree.
pub fn names_table(key: &str, schema: Option<&str>, sql_name: &str) -> bool {
    let schema = schema.unwrap_or("public");
    key == sql_name
        || key
            .strip_suffix(sql_name)
            .and_then(|qualifier| qualifier.strip_suffix('.'))
            .is_some_and(|qualifier| qualifier == schema)
}

/// Whether any column of the table has the enum type at `rust_path`, alone
/// or as an array element.
fn uses_type(table: &Table, rust_path: &str) -> bool {
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
fn resolve_value_lists(
    tables: &mut [Table],
    enums: &EnumRegistry,
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
                || generated
                    .iter()
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
struct EnumRegistry {
    entries: Vec<EnumEntry>,
}

struct EnumEntry {
    enum_type: EnumType,
    broken: Option<String>,
}

impl EnumEntry {
    /// The column type, or why the enum cannot be generated.
    fn rust_path(&self) -> std::result::Result<String, String> {
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
struct RenamedEnum {
    from: String,
    to: String,
}

/// An `ALTER TYPE` that broke an enum: every table using it fails.
struct BrokenEnum {
    rust_path: String,
    reason: String,
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

    fn lookup(&self, name: &ObjectName) -> Option<&EnumEntry> {
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
    fn define(&mut self, name: &ObjectName, labels: impl Iterator<Item = String>) {
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
    fn alter(&mut self, alter: &AlterType) -> std::result::Result<Option<RenamedEnum>, BrokenEnum> {
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
    fn drop(&mut self, name: &ObjectName) -> Option<String> {
        let index = self.position(name)?;
        Some(self.entries.remove(index).enum_type.rust_path())
    }

    /// The enums the `types` module generates.
    fn into_generatable(self) -> Vec<EnumType> {
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

/// `public` is on every default search path, so `public.x` is `x`.
fn normalize_schema(schema: Option<String>) -> Option<String> {
    schema.filter(|schema| schema != "public")
}

/// The known table an `ALTER TABLE` or `CREATE INDEX` names. An unqualified
/// name and a `public.` one refer to the same table.
fn find_table(tables: &[Table], name: &ObjectName) -> Option<usize> {
    let leaf = object_leaf_name(name);
    let schema = object_schema_name(name);
    tables.iter().position(|table| {
        table.sql_name == leaf && same_schema(table.schema.as_deref(), schema.as_deref())
    })
}

/// Whether two schema names are one schema: no schema is `public`.
fn same_schema(left: Option<&str>, right: Option<&str>) -> bool {
    fn normalize(schema: Option<&str>) -> Option<&str> {
        schema.filter(|schema| *schema != "public")
    }
    normalize(left) == normalize(right)
}

/// The relation names taken in `schema`: each known table's name and its key
/// constraints' (whose indexes are relations), and the `others`. PostgreSQL
/// gives an unnamed constraint a name no relation in its schema has.
fn relation_names(
    tables: &[Table],
    others: &OtherRelations,
    schema: Option<&str>,
) -> HashSet<String> {
    tables
        .iter()
        .filter(|table| same_schema(table.schema.as_deref(), schema))
        .flat_map(|table| {
            std::iter::once(table.sql_name.clone()).chain(
                table
                    .key_constraints
                    .iter()
                    .map(|constraint| constraint.name.clone()),
            )
        })
        .chain(others.in_schema(schema).cloned())
        .collect()
}

/// Relations that take a name in their schema besides the known tables and
/// their key constraints: indexes made by `CREATE INDEX` (named, or under
/// PostgreSQL's default `<table>_<columns>_idx`), sequences (`CREATE
/// SEQUENCE`, and a serial column's `<table>_<column>_seq`), and views and
/// materialized views, less any dropped since. Renames (`ALTER INDEX …
/// RENAME`) are not followed.
#[derive(Default)]
struct OtherRelations {
    /// Names by schema (`None` is `public`).
    names: HashMap<Option<String>, HashSet<String>>,
}

impl OtherRelations {
    fn in_schema(&self, schema: Option<&str>) -> impl Iterator<Item = &String> {
        self.names
            .get(&normalize_schema(schema.map(str::to_owned)))
            .into_iter()
            .flatten()
    }

    fn insert(&mut self, schema: Option<&str>, name: String) {
        self.names
            .entry(normalize_schema(schema.map(str::to_owned)))
            .or_default()
            .insert(name);
    }

    /// Records the relation `name` (schema-qualified or not) creates.
    fn create(&mut self, name: &ObjectName) {
        self.insert(object_schema_name(name).as_deref(), object_leaf_name(name));
    }

    /// Forgets a dropped relation.
    fn drop(&mut self, name: &ObjectName) {
        let schema = normalize_schema(object_schema_name(name));
        if let Some(names) = self.names.get_mut(&schema) {
            names.remove(&object_leaf_name(name));
        }
    }

    /// Records the sequences of the serial columns among `columns` that
    /// `table` gets, each under PostgreSQL's default name, and adds them to
    /// `taken`.
    fn create_sequences<'a>(
        &mut self,
        table: &ObjectName,
        columns: impl IntoIterator<Item = &'a ColumnDef>,
        taken: &mut HashSet<String>,
    ) {
        let schema = object_schema_name(table);
        let table = object_leaf_name(table);
        for column in columns {
            if !is_serial_type(&column.data_type) {
                continue;
            }
            let column = ident_name(&column.name);
            let name =
                choose_relation_name(&table, Some(&column), "seq", |name| taken.contains(name));
            taken.insert(name.clone());
            self.insert(schema.as_deref(), name);
        }
    }

    /// Records the index a `CREATE INDEX` makes: under its name, or under
    /// PostgreSQL's default one, which avoids the names `taken` in its schema.
    fn create_index(&mut self, index: &CreateIndex, taken: &HashSet<String>) {
        let schema = object_schema_name(&index.table_name);
        let name = match &index.name {
            Some(name) => object_leaf_name(name),
            None => {
                let columns: Vec<String> = index
                    .columns
                    .iter()
                    .map(|column| index_column_name(column).unwrap_or_else(|| "expr".to_owned()))
                    .collect();
                choose_relation_name(
                    &object_leaf_name(&index.table_name),
                    Some(&columns.join("_")),
                    "idx",
                    |name| taken.contains(name),
                )
            }
        };
        self.insert(schema.as_deref(), name);
    }
}

/// Applies a `DROP TABLE` to one name. A known table is removed (`removed`
/// sees it first), and so is every foreign key another table holds to it, as
/// `CASCADE` would: the referencing column stays, with its raw type. A table
/// that failed earlier is forgotten, failure included. Unknown names are
/// ignored, like `IF EXISTS`.
fn drop_table(
    tables: &mut Vec<Table>,
    failures: &mut Vec<TableFailure>,
    name: &ObjectName,
    removed: impl FnOnce(&Table),
) {
    let leaf = object_leaf_name(name);
    let schema = normalize_schema(object_schema_name(name));
    failures.retain(|failure| {
        failure.table != leaf || normalize_schema(failure.schema.clone()) != schema
    });
    let Some(index) = find_table(tables, name) else {
        return;
    };
    let table = tables.remove(index);
    removed(&table);
    for other in tables.iter_mut() {
        other.foreign_keys.retain(|key| {
            key.referenced_table != leaf
                || normalize_schema(key.referenced_schema.clone()) != schema
        });
    }
}

/// Moves every column of a renamed enum type (or an array of it) to the
/// enum's new path.
fn retype_columns(table: &mut Table, renamed: &RenamedEnum) {
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
fn drop_columns_of_type(table: &mut Table, rust_path: &str) {
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

/// Applies an `ALTER TABLE`'s clauses to a known table, in order. An error
/// names the clause that could not be applied.
fn apply_alter(
    table: &mut Table,
    operations: &[AlterTableOperation],
    enums: &EnumRegistry,
    taken: &HashSet<String>,
) -> std::result::Result<(), String> {
    for operation in operations {
        match operation {
            AlterTableOperation::AddColumn {
                if_not_exists,
                column_def,
                ..
            } => add_column(table, column_def, *if_not_exists, enums, taken)?,
            AlterTableOperation::DropColumn {
                column_names,
                if_exists,
                ..
            } => {
                for name in column_names {
                    drop_column(table, &ident_name(name), *if_exists)?;
                }
            }
            AlterTableOperation::RenameColumn {
                old_column_name,
                new_column_name,
            } => rename_column(
                table,
                &ident_name(old_column_name),
                &ident_name(new_column_name),
            )?,
            AlterTableOperation::AddConstraint { constraint, .. } => {
                add_constraint(table, constraint, taken)?;
            }
            AlterTableOperation::AlterColumn { column_name, op } => {
                alter_column(table, &ident_name(column_name), op, enums)?;
            }
            AlterTableOperation::EnableRowLevelSecurity
            | AlterTableOperation::DisableRowLevelSecurity => {}
            other => {
                return Err(format!("unsupported ALTER TABLE clause `{other}`"));
            }
        }
    }
    Ok(())
}

/// Applies an `ALTER COLUMN` change, leaving the column in its final shape.
/// Keys, unique keys, and foreign keys name their columns, so they follow a
/// type change.
///
/// PostgreSQL rejects `SET DEFAULT` and `DROP DEFAULT` on identity and
/// `GENERATED` columns, so a generated column they reach is a serial one:
/// replacing or dropping its `nextval` default leaves it no longer generated.
fn alter_column(
    table: &mut Table,
    name: &str,
    operation: &AlterColumnOperation,
    enums: &EnumRegistry,
) -> std::result::Result<(), String> {
    let index = column_index(table, name).ok_or_else(|| missing_column("ALTER COLUMN", name))?;
    let is_key = table.primary_key.iter().any(|key| key == name);
    let column = &mut table.columns[index];
    let column_fail = |reason: String| format!("ALTER COLUMN: column `{name}`: {reason}");
    match operation {
        AlterColumnOperation::SetDataType { data_type, .. } => {
            if is_serial_type(data_type) {
                return Err(column_fail(format!(
                    "type `{data_type}` exists only in CREATE TABLE and ADD COLUMN"
                )));
            }
            let (base_type, sqlx_type) = map_type_in(data_type, enums).map_err(column_fail)?;
            column.rust_type = rust_type(&base_type, column.nullable);
            column.sqlx_type = sqlx_type;
            // A value list restricts text only; one on another type is no enum.
            if column.base_type() != BaseType::Text {
                column.value_list = None;
            }
        }
        AlterColumnOperation::SetNotNull => set_nullable(column, false),
        AlterColumnOperation::DropNotNull => {
            if is_key {
                return Err(format!(
                    "ALTER COLUMN: column `{name}` is in the primary key, so it cannot drop NOT NULL"
                ));
            }
            set_nullable(column, true);
        }
        AlterColumnOperation::SetDefault { .. } => {
            column.has_default = true;
            column.is_generated = false;
        }
        AlterColumnOperation::DropDefault => {
            column.has_default = false;
            column.is_generated = false;
        }
        AlterColumnOperation::AddGenerated { .. } => {
            if column.nullable {
                return Err(format!(
                    "ALTER COLUMN: column `{name}` must be NOT NULL before it can become an identity column"
                ));
            }
            column.has_default = true;
            column.is_generated = true;
        }
    }
    Ok(())
}

/// Makes `column` nullable or required, wrapping or unwrapping its Rust type.
fn set_nullable(column: &mut Column, nullable: bool) {
    if column.nullable == nullable {
        return;
    }
    let base_type = if column.nullable {
        column
            .rust_type
            .strip_prefix("Option<")
            .and_then(|inner| inner.strip_suffix('>'))
            .unwrap_or(&column.rust_type)
            .to_owned()
    } else {
        column.rust_type.clone()
    };
    column.rust_type = rust_type(&base_type, nullable);
    column.nullable = nullable;
}

fn column_index(table: &Table, name: &str) -> Option<usize> {
    table
        .columns
        .iter()
        .position(|column| column.sql_name == name)
}

fn missing_column(clause: &str, name: &str) -> String {
    format!("{clause}: column `{name}` does not exist")
}

fn add_column(
    table: &mut Table,
    column_def: &ColumnDef,
    if_not_exists: bool,
    enums: &EnumRegistry,
    taken: &HashSet<String>,
) -> std::result::Result<(), String> {
    let name = ident_name(&column_def.name);
    if column_index(table, &name).is_some() {
        if if_not_exists {
            return Ok(());
        }
        return Err(format!("ADD COLUMN: column `{name}` already exists"));
    }
    let built =
        build_column(column_def, enums).map_err(|reason| format!("ADD COLUMN: {reason}"))?;
    if table
        .columns
        .iter()
        .any(|column| column.rust_name == built.raw.rust_name)
    {
        return Err(format!("ADD COLUMN: {}", field_taken(&built.raw)));
    }
    if let Some(constraint) = &built.primary_key {
        if !table.primary_key.is_empty() {
            return Err(format!(
                "ADD COLUMN: column `{name}` adds a second primary key"
            ));
        }
        table.primary_key = vec![name.clone()];
        add_key_constraint(table, constraint.clone(), vec![name.clone()], true, taken);
    }
    if let Some(constraint) = &built.unique {
        table.unique_keys.push(UniqueKey::new(vec![name.clone()]));
        add_key_constraint(table, constraint.clone(), vec![name.clone()], false, taken);
    }
    table.foreign_keys.extend(built.foreign_key);
    table
        .columns
        .push(built.raw.into_column(built.primary_key.is_some()));
    for (column, labels) in built.value_lists {
        if let Some(index) = column_index(table, &column) {
            restrict_to_values(&mut table.columns[index], labels);
        }
    }
    Ok(())
}

/// Drops a column. Like PostgreSQL, dropping a key column drops the primary
/// key, unique keys, and foreign keys it belongs to.
fn drop_column(table: &mut Table, name: &str, if_exists: bool) -> std::result::Result<(), String> {
    let Some(index) = column_index(table, name) else {
        if if_exists {
            return Ok(());
        }
        return Err(missing_column("DROP COLUMN", name));
    };
    table.columns.remove(index);
    if table.primary_key.iter().any(|key| key == name) {
        table.primary_key.clear();
        for column in &mut table.columns {
            column.is_primary_key = false;
        }
    }
    table
        .unique_keys
        .retain(|key| !key.columns.iter().any(|column| column == name));
    table
        .key_constraints
        .retain(|key| !key.columns.iter().any(|column| column == name));
    table.foreign_keys.retain(|key| key.column != name);
    Ok(())
}

fn rename_column(table: &mut Table, old: &str, new: &str) -> std::result::Result<(), String> {
    let index = column_index(table, old).ok_or_else(|| missing_column("RENAME COLUMN", old))?;
    if column_index(table, new).is_some() {
        return Err(format!("RENAME COLUMN: column `{new}` already exists"));
    }
    let rust_name =
        naming::rust_identifier(new).map_err(|reason| format!("RENAME COLUMN: {reason}"))?;
    if let Some(other) = table
        .columns
        .iter()
        .find(|column| column.sql_name != old && column.rust_name == rust_name)
    {
        return Err(format!(
            "RENAME COLUMN: column `{new}` maps to Rust field `{rust_name}`, which is already taken by column `{}`",
            other.sql_name
        ));
    }
    let column = &mut table.columns[index];
    column.sql_name = new.to_owned();
    column.rust_name = rust_name;

    let rename = |column: &mut String| {
        if column == old {
            *column = new.to_owned();
        }
    };
    table.primary_key.iter_mut().for_each(rename);
    for key in &mut table.unique_keys {
        key.columns.iter_mut().for_each(rename);
    }
    // PostgreSQL keeps a constraint's name when a column is renamed.
    for key in &mut table.key_constraints {
        key.columns.iter_mut().for_each(rename);
    }
    for key in &mut table.foreign_keys {
        rename(&mut key.column);
    }
    Ok(())
}

/// Adds a primary key, unique, or foreign key constraint, or a value-list
/// check. Like in `CREATE TABLE`, any other check leaves the model unchanged.
/// Other constraint kinds fail, so a constraint that might change the model
/// is never dropped silently.
fn add_constraint(
    table: &mut Table,
    constraint: &TableConstraint,
    taken: &HashSet<String>,
) -> std::result::Result<(), String> {
    let clause = |reason: String| format!("ADD CONSTRAINT: {reason}");
    let known_columns = |table: &Table, keys: &[String]| match keys
        .iter()
        .find(|key| column_index(table, key).is_none())
    {
        Some(key) => Err(clause(format!("column `{key}` does not exist"))),
        None => Ok(()),
    };
    match constraint {
        TableConstraint::PrimaryKey { name, columns, .. } => {
            let keys = key_columns(columns, "PRIMARY KEY").map_err(clause)?;
            if !table.primary_key.is_empty() {
                return Err(clause("the table already has a primary key".to_owned()));
            }
            known_columns(table, &keys)?;
            for column in &mut table.columns {
                if keys.contains(&column.sql_name) {
                    column.is_primary_key = true;
                    column.nullable = false;
                    column.rust_type = rust_type(&column.sqlx_type, false);
                }
            }
            add_key_constraint(
                table,
                name.as_ref().map(ident_name),
                keys.clone(),
                true,
                taken,
            );
            table.primary_key = keys;
        }
        TableConstraint::Unique { name, columns, .. } => {
            let keys = key_columns(columns, "UNIQUE").map_err(clause)?;
            known_columns(table, &keys)?;
            add_key_constraint(
                table,
                name.as_ref().map(ident_name),
                keys.clone(),
                false,
                taken,
            );
            table.unique_keys.push(UniqueKey::new(keys));
        }
        TableConstraint::ForeignKey { columns, .. } => {
            let keys: Vec<String> = columns.iter().map(ident_name).collect();
            known_columns(table, &keys)?;
            table
                .foreign_keys
                .extend(constraint_foreign_key(constraint));
        }
        // Like in `CREATE TABLE`, a value list on a text column restricts it,
        // and any other check leaves the model unchanged.
        TableConstraint::Check { expr, .. } => {
            if let Some((name, labels)) = value_list_check(expr)
                && let Some(index) = column_index(table, &name)
            {
                restrict_to_values(&mut table.columns[index], labels);
            }
        }
        other => return Err(format!("unsupported ALTER TABLE clause `ADD {other}`")),
    }
    Ok(())
}

/// Records a `CREATE TRIGGER` on a known table: a row-level `BEFORE` trigger
/// that fires on `INSERT` (alone or with other events) can supply the values
/// of the columns inserts leave out. Its function is not inspected. A
/// `CREATE OR REPLACE` of a trigger with the same name replaces it.
fn record_trigger(tables: &mut [Table], trigger: &CreateTrigger) {
    let Some(index) = find_table(tables, &trigger.table_name) else {
        return;
    };
    let table = &mut tables[index];
    let name = object_leaf_name(&trigger.name);
    table.before_insert_triggers.retain(|other| *other != name);
    let before_insert = trigger.period == TriggerPeriod::Before
        && trigger.trigger_object == TriggerObject::Row
        && trigger.events.contains(&TriggerEvent::Insert);
    if before_insert {
        table.before_insert_triggers.push(name);
    }
}

/// Applies a `DROP TRIGGER`: the trigger no longer supplies values. Without
/// a table name, a trigger of that name is dropped from every table.
fn forget_trigger(tables: &mut [Table], trigger: &DropTrigger) {
    let name = object_leaf_name(&trigger.trigger_name);
    let only = trigger
        .table_name
        .as_ref()
        .and_then(|table| find_table(tables, table));
    if trigger.table_name.is_some() && only.is_none() {
        return;
    }
    for (index, table) in tables.iter_mut().enumerate() {
        if only.is_none_or(|only| only == index) {
            table.before_insert_triggers.retain(|other| *other != name);
        }
    }
}

/// Records a `CREATE UNIQUE INDEX` on plain columns of a known table as a
/// unique key, keeping a partial index's condition. Expression indexes and
/// non-unique indexes change nothing.
fn record_unique_index(tables: &mut [Table], index: &CreateIndex) {
    if !index.unique {
        return;
    }
    let Some(position) = find_table(tables, &index.table_name) else {
        return;
    };
    let Some(columns) = index
        .columns
        .iter()
        .map(index_column_name)
        .collect::<Option<Vec<String>>>()
    else {
        return;
    };
    let table = &mut tables[position];
    if columns
        .iter()
        .any(|column| column_index(table, column).is_none())
    {
        return;
    }
    table.unique_keys.push(UniqueKey {
        columns,
        predicate: index.predicate.as_ref().map(ToString::to_string),
    });
}

fn build_table(
    create: &CreateTable,
    enums: &EnumRegistry,
    taken: &HashSet<String>,
) -> std::result::Result<Table, TableFailure> {
    let sql_name = object_leaf_name(&create.name);
    let fail = |reason: String| table_failure(&sql_name, reason);

    if create.columns.is_empty() {
        return Err(fail("table has no columns".to_owned()));
    }

    let mut raw_columns: Vec<RawColumn> = Vec::new();
    let mut seen_fields: HashSet<String> = HashSet::new();
    let mut foreign_keys = Vec::new();
    let mut unique_keys = Vec::new();
    // `(name, columns, primary)`, named once every constraint is known.
    let mut constraints: Vec<(Option<String>, Vec<String>, bool)> = Vec::new();
    let mut inline_primary_keys: Vec<String> = Vec::new();
    let mut value_lists: Vec<(String, Vec<String>)> = Vec::new();

    for column_def in &create.columns {
        let built = build_column(column_def, enums).map_err(fail)?;
        if !seen_fields.insert(built.raw.rust_name.clone()) {
            return Err(fail(field_taken(&built.raw)));
        }
        if let Some(name) = &built.primary_key {
            inline_primary_keys.push(built.raw.sql_name.clone());
            constraints.push((name.clone(), vec![built.raw.sql_name.clone()], true));
        }
        if let Some(name) = &built.unique {
            unique_keys.push(UniqueKey::new(vec![built.raw.sql_name.clone()]));
            constraints.push((name.clone(), vec![built.raw.sql_name.clone()], false));
        }
        foreign_keys.extend(built.foreign_key);
        value_lists.extend(built.value_lists);
        raw_columns.push(built.raw);
    }

    if inline_primary_keys.len() > 1 {
        return Err(fail(format!(
            "multiple inline primary keys ({}) are not supported",
            inline_primary_keys.join(", ")
        )));
    }

    let mut table_primary_key: Vec<String> = Vec::new();
    for constraint in &create.constraints {
        match constraint {
            TableConstraint::PrimaryKey {
                name,
                columns: keys,
                ..
            } => {
                table_primary_key = key_columns(keys, "PRIMARY KEY").map_err(fail)?;
                let name = name.as_ref().map(ident_name);
                constraints.push((name, table_primary_key.clone(), true));
            }
            TableConstraint::Unique {
                name,
                columns: keys,
                ..
            } => {
                if let Ok(keys) = key_columns(keys, "UNIQUE") {
                    unique_keys.push(UniqueKey::new(keys.clone()));
                    constraints.push((name.as_ref().map(ident_name), keys, false));
                }
            }
            TableConstraint::Check { expr, .. } => value_lists.extend(value_list_check(expr)),
            other => foreign_keys.extend(constraint_foreign_key(other)),
        }
    }

    let primary_key = if table_primary_key.is_empty() {
        inline_primary_keys
    } else {
        table_primary_key
    };

    for key in &primary_key {
        if !raw_columns.iter().any(|raw| raw.sql_name == *key) {
            return Err(fail(format!(
                "primary key `{key}` does not match any column"
            )));
        }
    }

    let mut columns: Vec<Column> = raw_columns
        .into_iter()
        .map(|raw| {
            let is_primary_key = primary_key.contains(&raw.sql_name);
            raw.into_column(is_primary_key)
        })
        .collect();
    for (name, labels) in value_lists {
        if let Some(column) = columns.iter_mut().find(|column| column.sql_name == name) {
            restrict_to_values(column, labels);
        }
    }

    let module_name = naming::rust_identifier(&sql_name).map_err(fail)?;
    let struct_name = naming::struct_name(&module_name);

    let mut table = Table {
        schema: object_schema_name(&create.name),
        sql_name,
        module_name,
        struct_name,
        columns,
        primary_key,
        foreign_keys,
        unique_keys,
        key_constraints: Vec::new(),
        version_column: None,
        soft_delete_column: None,
        tenant_column: None,
        relationships: Vec::new(),
        before_insert_triggers: Vec::new(),
    };
    // Like PostgreSQL, one `CREATE TABLE` makes one constraint of several
    // over the same column list: a primary key if any of them is, named by
    // the first that has a name.
    let mut merged: Vec<(Option<String>, Vec<String>, bool)> = Vec::new();
    for (name, columns, primary) in constraints {
        match merged
            .iter_mut()
            .find(|(_, earlier, _)| *earlier == columns)
        {
            Some((earlier_name, _, earlier_primary)) => {
                *earlier_primary |= primary;
                if earlier_name.is_none() {
                    *earlier_name = name;
                }
            }
            None => merged.push((name, columns, primary)),
        }
    }
    for (name, columns, primary) in merged {
        add_key_constraint(&mut table, name, columns, primary, taken);
    }
    Ok(table)
}

/// Records a primary key or unique constraint under its declared name, or
/// under the name PostgreSQL gives an unnamed one (see
/// [`default_constraint_name`]). `elsewhere` holds the relation names other
/// tables of the schema take (see [`relation_names`]).
fn add_key_constraint(
    table: &mut Table,
    name: Option<String>,
    columns: Vec<String>,
    primary: bool,
    elsewhere: &HashSet<String>,
) {
    let name = name.unwrap_or_else(|| {
        let taken = |candidate: &str| {
            candidate == table.sql_name
                || elsewhere.contains(candidate)
                || table
                    .key_constraints
                    .iter()
                    .any(|constraint| constraint.name == candidate)
        };
        default_constraint_name(&table.sql_name, &columns, primary, taken)
    });
    table.key_constraints.push(KeyConstraint { name, columns });
}

/// PostgreSQL's longest identifier, in bytes (`NAMEDATALEN - 1`).
const MAX_IDENTIFIER_BYTES: usize = 63;

/// The name PostgreSQL gives an unnamed primary key (`<table>_pkey`) or
/// unique constraint (`<table>_<col>_<col>_key`); see
/// [`choose_relation_name`].
fn default_constraint_name(
    table: &str,
    columns: &[String],
    primary: bool,
    taken: impl Fn(&str) -> bool,
) -> String {
    if primary {
        choose_relation_name(table, None, "pkey", taken)
    } else {
        choose_relation_name(table, Some(&columns.join("_")), "key", taken)
    }
}

/// PostgreSQL's `ChooseRelationName`: `name1[_name2]_label`, with the parts
/// shortened, the longer one first, until the name fits in 63 bytes, and a
/// number after the label (`_key1`) while the name is `taken`. PostgreSQL
/// avoids every relation name in the schema; `taken` says which are.
fn choose_relation_name(
    name1: &str,
    name2: Option<&str>,
    label: &str,
    taken: impl Fn(&str) -> bool,
) -> String {
    let mut pass = 0;
    loop {
        let label = if pass == 0 {
            label.to_owned()
        } else {
            format!("{label}{pass}")
        };
        let name = object_name(name1, name2, &label);
        if !taken(&name) {
            return name;
        }
        pass += 1;
    }
}

/// PostgreSQL's `makeObjectName`: `name1[_name2]_label`, with `name1` and
/// `name2` shortened (the longer one first, on character boundaries) so the
/// whole fits in [`MAX_IDENTIFIER_BYTES`].
fn object_name(name1: &str, name2: Option<&str>, label: &str) -> String {
    let overhead = label.len() + 1 + usize::from(name2.is_some());
    let available = MAX_IDENTIFIER_BYTES.saturating_sub(overhead);
    let (mut len1, mut len2) = (name1.len(), name2.map_or(0, str::len));
    while len1 + len2 > available {
        if len1 > len2 {
            len1 -= 1;
        } else {
            len2 -= 1;
        }
    }
    let clip = |name: &str, mut len: usize| {
        while !name.is_char_boundary(len) {
            len -= 1;
        }
        name[..len].to_owned()
    };
    let mut name = clip(name1, len1);
    if let Some(name2) = name2 {
        name.push('_');
        name.push_str(&clip(name2, len2));
    }
    name.push('_');
    name.push_str(label);
    name
}

/// A column definition from `CREATE TABLE` or `ALTER TABLE … ADD COLUMN`,
/// with the constraints declared inline on it.
struct BuiltColumn {
    raw: RawColumn,
    /// `Some` for an inline `PRIMARY KEY`, holding its declared name.
    primary_key: Option<Option<String>>,
    /// `Some` for an inline `UNIQUE`, holding its declared name.
    unique: Option<Option<String>>,
    foreign_key: Option<ForeignKey>,
    /// Value-list `CHECK`s declared inline, by the column they name (usually
    /// this one, but PostgreSQL lets a column check name any column).
    value_lists: Vec<(String, Vec<String>)>,
}

fn build_column(
    column_def: &ColumnDef,
    enums: &EnumRegistry,
) -> std::result::Result<BuiltColumn, String> {
    let column_name = ident_name(&column_def.name);
    let column_fail = |reason: String| format!("column `{column_name}`: {reason}");

    // A serial column is `NOT NULL` with a sequence default, on or off the
    // primary key.
    let mut is_generated = is_serial_type(&column_def.data_type);
    let mut has_default = is_generated;
    let mut nullable = !is_generated;
    let mut primary_key = None;
    let mut unique = None;
    let mut foreign_key = None;
    let mut value_lists = Vec::new();
    for option in &column_def.options {
        match &option.option {
            ColumnOption::NotNull => nullable = false,
            ColumnOption::Unique { is_primary, .. } => {
                let name = Some(option.name.as_ref().map(ident_name));
                if *is_primary {
                    primary_key = name;
                } else {
                    unique = name;
                }
            }
            ColumnOption::ForeignKey {
                foreign_table,
                referred_columns,
                ..
            } if referred_columns.len() == 1 => {
                foreign_key = Some(ForeignKey {
                    column: column_name.clone(),
                    referenced_schema: object_schema_name(foreign_table),
                    referenced_table: object_leaf_name(foreign_table),
                    referenced_column: ident_name(&referred_columns[0]),
                });
            }
            ColumnOption::Default(_) => has_default = true,
            ColumnOption::Check(expr) => value_lists.extend(value_list_check(expr)),
            ColumnOption::Generated { .. } | ColumnOption::Identity(_) => {
                has_default = true;
                is_generated = true;
            }
            _ => {}
        }
    }

    let (base_type, sqlx_type) = map_type_in(&column_def.data_type, enums).map_err(column_fail)?;
    let rust_name = naming::rust_identifier(&column_name).map_err(column_fail)?;

    Ok(BuiltColumn {
        raw: RawColumn {
            sql_name: column_name,
            rust_name,
            base_type,
            sqlx_type,
            nullable,
            has_default,
            is_generated,
        },
        primary_key,
        unique,
        foreign_key,
        value_lists,
    })
}

/// The column and values of a value-list check, `col IN ('a', 'b', …)`
/// (parenthesized or not), with duplicate values dropped. Any other shape,
/// including `NOT IN` and lists holding anything but string literals, is
/// `None`.
fn value_list_check(expr: &Expr) -> Option<(String, Vec<String>)> {
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
fn restrict_to_values(column: &mut Column, labels: Vec<String>) {
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

fn field_taken(raw: &RawColumn) -> String {
    format!(
        "column `{}` maps to Rust field `{}`, which is already taken by another column",
        raw.sql_name, raw.rust_name
    )
}

/// The plain column names of a `PRIMARY KEY` or `UNIQUE` column list.
fn key_columns(keys: &[IndexColumn], what: &str) -> std::result::Result<Vec<String>, String> {
    let mut names = Vec::with_capacity(keys.len());
    for key in keys {
        let name = index_column_name(key)
            .ok_or_else(|| format!("{what} constraint contains a non-column expression"))?;
        if names.contains(&name) {
            return Err(format!("{what} lists column `{name}` more than once"));
        }
        names.push(name);
    }
    Ok(names)
}

/// A single-column `FOREIGN KEY` table constraint; composite keys are not
/// recorded.
fn constraint_foreign_key(constraint: &TableConstraint) -> Option<ForeignKey> {
    match constraint {
        TableConstraint::ForeignKey {
            columns,
            foreign_table,
            referred_columns,
            ..
        } if columns.len() == 1 && referred_columns.len() == 1 => Some(ForeignKey {
            column: ident_name(&columns[0]),
            referenced_schema: object_schema_name(foreign_table),
            referenced_table: object_leaf_name(foreign_table),
            referenced_column: ident_name(&referred_columns[0]),
        }),
        _ => None,
    }
}

fn table_failure(table: &str, reason: String) -> TableFailure {
    TableFailure {
        table: table.to_owned(),
        reason,
        path: None,
        line: None,
        schema: None,
    }
}

struct RawColumn {
    sql_name: String,
    rust_name: String,
    base_type: String,
    sqlx_type: String,
    nullable: bool,
    has_default: bool,
    is_generated: bool,
}

impl RawColumn {
    fn into_column(mut self, is_primary_key: bool) -> Column {
        if is_primary_key {
            self.nullable = false;
        }
        Column {
            rust_type: rust_type(&self.base_type, self.nullable),
            sql_name: self.sql_name,
            rust_name: self.rust_name,
            sqlx_type: self.sqlx_type,
            nullable: self.nullable,
            is_primary_key,
            has_default: self.has_default,
            is_generated: self.is_generated,
            value_list: None,
            typed_id: None,
            is_managed: false,
        }
    }
}

fn rust_type(base_type: &str, nullable: bool) -> String {
    if nullable {
        format!("Option<{base_type}>")
    } else {
        base_type.to_owned()
    }
}

pub(crate) fn ident_name(ident: &Ident) -> String {
    match ident.quote_style {
        Some('"') => ident.value.clone(),
        _ => ident.value.to_lowercase(),
    }
}

pub(crate) fn object_leaf_name(name: &ObjectName) -> String {
    name.0
        .iter()
        .rev()
        .find_map(|part| match part {
            ObjectNamePart::Identifier(ident) => Some(ident_name(ident)),
            ObjectNamePart::Function(_) => None,
        })
        .unwrap_or_default()
}

/// The schema qualifier of a possibly qualified name (`catalog` in
/// `catalog.products`); `None` for an unqualified name.
fn object_schema_name(name: &ObjectName) -> Option<String> {
    let identifiers: Vec<&Ident> = name
        .0
        .iter()
        .filter_map(|part| match part {
            ObjectNamePart::Identifier(ident) => Some(ident),
            ObjectNamePart::Function(_) => None,
        })
        .collect();
    match identifiers.as_slice() {
        [.., schema, _table] => Some(ident_name(schema)),
        _ => None,
    }
}

fn index_column_name(column: &IndexColumn) -> Option<String> {
    match &column.column.expr {
        Expr::Identifier(ident) => Some(ident_name(ident)),
        _ => None,
    }
}

/// Maps a query-file cast's SQL type to its Rust type, resolving the
/// database enum types among the generated `enums` (a value-list enum names
/// no database type).
pub(crate) fn map_type(
    data_type: &DataType,
    enums: &[EnumType],
) -> std::result::Result<(String, String), String> {
    let registry = EnumRegistry {
        entries: enums
            .iter()
            .filter(|enum_type| enum_type.column.is_none())
            .map(|enum_type| EnumEntry {
                enum_type: enum_type.clone(),
                broken: None,
            })
            .collect(),
    };
    map_type_in(data_type, &registry)
}

/// Maps a SQL type to `(Rust type, base type)`, resolving enum types defined
/// so far.
fn map_type_in(
    data_type: &DataType,
    enums: &EnumRegistry,
) -> std::result::Result<(String, String), String> {
    let base = match data_type {
        DataType::SmallInt(_) | DataType::Int2(_) => "i16",
        DataType::Int(_) | DataType::Int4(_) | DataType::Integer(_) => "i32",
        DataType::BigInt(_) | DataType::Int8(_) => "i64",
        DataType::Bool | DataType::Boolean => "bool",
        DataType::Real => "f32",
        DataType::Float8 | DataType::DoublePrecision | DataType::Double(_) | DataType::Float(_) => {
            "f64"
        }
        DataType::Character(_)
        | DataType::Char(_)
        | DataType::CharacterVarying(_)
        | DataType::Varchar(_)
        | DataType::Text
        | DataType::String(_) => "String",
        DataType::Uuid => "uuid::Uuid",
        DataType::Date => "chrono::NaiveDate",
        DataType::Time(_, TimezoneInfo::None | TimezoneInfo::WithoutTimeZone) => {
            "chrono::NaiveTime"
        }
        DataType::Timestamp(_, TimezoneInfo::None | TimezoneInfo::WithoutTimeZone) => {
            "chrono::NaiveDateTime"
        }
        DataType::Timestamp(_, TimezoneInfo::Tz | TimezoneInfo::WithTimeZone) => {
            "chrono::DateTime<chrono::Utc>"
        }
        DataType::Time(_, TimezoneInfo::Tz | TimezoneInfo::WithTimeZone) => rust_type::TIME_TZ,
        DataType::Interval { .. } => rust_type::INTERVAL,
        DataType::Bytea => rust_type::BYTES,
        DataType::JSON | DataType::JSONB => "serde_json::Value",
        DataType::Numeric(info) | DataType::Decimal(info) => {
            check_decimal_precision(info)?;
            "rust_decimal::Decimal"
        }
        DataType::Array(element) => {
            let inner = array_element_type(element)?;
            let (base, _) = map_type_in(inner, enums)?;
            return Ok((format!("Vec<{base}>"), format!("Vec<{base}>")));
        }
        DataType::Custom(name, _) => match enums.lookup(name) {
            Some(enum_type) => {
                let path = enum_type.rust_path()?;
                return Ok((path.clone(), path));
            }
            None => custom_type_leaf(name)?,
        },
        other => return Err(format!("type `{other}` is not supported yet")),
    };

    Ok((base.to_owned(), base.to_owned()))
}

fn custom_type_leaf(name: &ObjectName) -> std::result::Result<&'static str, String> {
    match object_leaf_name(name).to_ascii_uppercase().as_str() {
        "SERIAL" => Ok("i32"),
        "BIGSERIAL" => Ok("i64"),
        "SMALLSERIAL" => Ok("i16"),
        "FLOAT4" => Ok("f32"),
        other => Err(format!("type `{other}` is not supported yet")),
    }
}

fn is_serial_type(data_type: &DataType) -> bool {
    matches!(
        data_type,
        DataType::Custom(name, _)
            if matches!(
                object_leaf_name(name).to_ascii_uppercase().as_str(),
                "SERIAL" | "BIGSERIAL" | "SMALLSERIAL"
            )
    )
}

fn array_element_type(element: &ArrayElemTypeDef) -> std::result::Result<&DataType, String> {
    match element {
        ArrayElemTypeDef::SquareBracket(inner, _)
        | ArrayElemTypeDef::AngleBracket(inner)
        | ArrayElemTypeDef::Parenthesis(inner) => Ok(inner),
        ArrayElemTypeDef::None => Err("array type has no element type".to_owned()),
    }
}

const DECIMAL_MAX_PRECISION: u64 = 28;

fn check_decimal_precision(info: &ExactNumberInfo) -> std::result::Result<(), String> {
    let precision = match info {
        ExactNumberInfo::None => return Ok(()),
        ExactNumberInfo::Precision(precision)
        | ExactNumberInfo::PrecisionAndScale(precision, _) => *precision,
    };
    if precision > DECIMAL_MAX_PRECISION {
        return Err(format!(
            "NUMERIC precision {precision} exceeds rust_decimal's {DECIMAL_MAX_PRECISION} significant digits"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(sql: &str) -> ParsedSchema {
        parse_schema(sql).expect("schema should parse")
    }

    fn one_table(sql: &str) -> Table {
        let parsed = parse(sql);
        assert!(
            parsed.failures.is_empty(),
            "unexpected failures: {:?}",
            parsed.failures
        );
        assert_eq!(parsed.schema.tables.len(), 1);
        parsed.schema.tables[0].clone()
    }

    #[test]
    fn parses_simple_table() {
        let table = one_table("CREATE TABLE users (id UUID PRIMARY KEY, email TEXT NOT NULL);");
        assert_eq!(table.sql_name, "users");
        assert_eq!(table.primary_key, ["id"]);
        assert!(table.columns[0].is_primary_key);
        assert!(!table.columns[1].nullable);
    }

    #[test]
    fn names_modules_and_structs_mechanically() {
        let table = one_table("CREATE TABLE user_profiles (id BIGSERIAL PRIMARY KEY);");
        assert_eq!(table.module_name, "user_profiles");
        assert_eq!(table.struct_name, "UserProfile");
    }

    #[test]
    fn detects_table_level_primary_key() {
        let table = one_table("CREATE TABLE items (a INT NOT NULL, PRIMARY KEY (a));");
        assert_eq!(table.primary_key, ["a"]);
    }

    #[test]
    fn accepts_composite_primary_key_in_declaration_order() {
        let table = one_table(
            "CREATE TABLE order_items (a INT NOT NULL, b INT NOT NULL, c TEXT, PRIMARY KEY (a, b));",
        );
        assert_eq!(table.primary_key, ["a", "b"]);
        assert!(table.columns[0].is_primary_key);
        assert!(table.columns[1].is_primary_key);
        assert!(!table.columns[2].is_primary_key);
        assert!(!table.columns[0].nullable);
        assert!(!table.columns[1].nullable);
    }

    #[test]
    fn rejects_primary_key_listing_unknown_column() {
        let parsed = parse("CREATE TABLE items (a INT, b INT, PRIMARY KEY (a, c));");
        assert_eq!(parsed.schema.tables.len(), 0);
        assert!(
            parsed.failures[0]
                .reason
                .contains("does not match any column")
        );
    }

    #[test]
    fn rejects_primary_key_listing_duplicate_column() {
        let parsed = parse("CREATE TABLE items (a INT, PRIMARY KEY (a, a));");
        assert_eq!(parsed.schema.tables.len(), 0);
        assert!(parsed.failures[0].reason.contains("more than once"));
    }

    #[test]
    fn rejects_unsupported_type() {
        let parsed = parse("CREATE TABLE shifts (id BIGSERIAL PRIMARY KEY, span INET NOT NULL);");
        assert_eq!(parsed.schema.tables.len(), 0);
        assert!(parsed.failures[0].reason.contains("not supported"));
    }

    #[test]
    fn maps_numeric_and_decimal_columns() {
        let table = one_table(
            "CREATE TABLE money (amount NUMERIC(12, 2) NOT NULL, fee DECIMAL, rate NUMERIC(6,3));",
        );
        assert_eq!(table.columns[0].rust_type, "rust_decimal::Decimal");
        assert_eq!(table.columns[1].rust_type, "Option<rust_decimal::Decimal>");
        assert_eq!(table.columns[2].rust_type, "Option<rust_decimal::Decimal>");
    }

    #[test]
    fn rejects_decimal_precision_beyond_rust_decimal() {
        let parsed = parse("CREATE TABLE money (amount NUMERIC(50, 10) NOT NULL);");
        assert_eq!(parsed.schema.tables.len(), 0);
        assert!(parsed.failures[0].reason.contains("exceeds"));
    }

    #[test]
    fn folds_unquoted_case_and_keeps_quoted() {
        let table =
            one_table("CREATE TABLE \"Users\" (ID UUID PRIMARY KEY, \"Email\" TEXT NOT NULL);");
        assert_eq!(table.sql_name, "Users");
        assert_eq!(table.columns[0].sql_name, "id");
        assert_eq!(table.columns[1].sql_name, "Email");
    }

    #[test]
    fn keeps_schema_qualification() {
        let table = one_table("CREATE TABLE public.users (id BIGSERIAL PRIMARY KEY);");
        assert_eq!(table.sql_name, "users");
        assert_eq!(table.schema.as_deref(), Some("public"));
        assert_eq!(table.display_name(), "public.users");
    }

    #[test]
    fn maps_serials_and_timestamps() {
        let table = one_table(
            "CREATE TABLE events (a SERIAL PRIMARY KEY, b BIGSERIAL, c TIMESTAMPTZ NOT NULL, d TIMESTAMP, e VARCHAR(255));",
        );
        assert_eq!(table.columns[0].sqlx_type, "i32");
        assert_eq!(table.columns[1].sqlx_type, "i64");
        assert_eq!(table.columns[2].rust_type, "chrono::DateTime<chrono::Utc>");
        assert_eq!(table.columns[3].rust_type, "Option<chrono::NaiveDateTime>");
        assert_eq!(table.columns[4].rust_type, "Option<String>");
    }

    #[test]
    fn flags_serial_columns_as_generated_with_default() {
        let table = one_table("CREATE TABLE events (id BIGSERIAL PRIMARY KEY, note TEXT);");
        assert!(table.columns[0].is_generated);
        assert!(table.columns[0].has_default);
        assert!(!table.columns[1].is_generated);
        assert!(!table.columns[1].has_default);
    }

    #[test]
    fn flags_default_clause_columns() {
        let table = one_table(
            "CREATE TABLE events (id UUID PRIMARY KEY, created_at TIMESTAMPTZ NOT NULL DEFAULT now(), note TEXT);",
        );
        assert!(table.columns[1].has_default);
        assert!(!table.columns[1].is_generated);
        assert!(!table.columns[2].has_default);
    }

    #[test]
    fn flags_generated_and_identity_columns() {
        let table = one_table(
            "CREATE TABLE events (id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY, total INT GENERATED ALWAYS AS (1) STORED);",
        );
        assert!(table.columns[0].is_generated);
        assert!(table.columns[0].has_default);
        assert!(table.columns[1].is_generated);
        assert!(table.columns[1].has_default);
    }

    #[test]
    fn maps_time_and_arrays() {
        let table = one_table(
            "CREATE TABLE shifts (starts_at TIME NOT NULL, ends_at TIME, role_keys TEXT[] NOT NULL, weekday INTEGER[], branch_ids UUID[]);",
        );
        assert_eq!(table.columns[0].rust_type, "chrono::NaiveTime");
        assert_eq!(table.columns[1].rust_type, "Option<chrono::NaiveTime>");
        assert_eq!(table.columns[2].rust_type, "Vec<String>");
        assert_eq!(table.columns[3].rust_type, "Option<Vec<i32>>");
        assert_eq!(table.columns[4].rust_type, "Option<Vec<uuid::Uuid>>");
    }

    #[test]
    fn maps_interval_bytea_and_time_with_time_zone() {
        let table = one_table(
            "CREATE TABLE shifts (span INTERVAL NOT NULL, spans INTERVAL[], badge BYTEA NOT NULL, \
             photos BYTEA[], opens TIME WITH TIME ZONE NOT NULL, closes TIMETZ, slots TIMETZ[]);",
        );
        let types: Vec<&str> = table
            .columns
            .iter()
            .map(|column| column.rust_type.as_str())
            .collect();
        assert_eq!(
            types,
            [
                "sqlx::postgres::types::PgInterval",
                "Option<Vec<sqlx::postgres::types::PgInterval>>",
                "Vec<u8>",
                "Option<Vec<Vec<u8>>>",
                "sqlx::postgres::types::PgTimeTz",
                "Option<sqlx::postgres::types::PgTimeTz>",
                "Option<Vec<sqlx::postgres::types::PgTimeTz>>",
            ]
        );
        // SQLx implements no `PartialOrd` for `PgInterval` or `PgTimeTz`, so
        // they get equality filters, as `BYTEA` does; arrays get none.
        let equality = Some(crate::schema::FilterKind::Equality);
        let kinds: Vec<_> = table
            .columns
            .iter()
            .map(|column| column.filter_kind())
            .collect();
        assert_eq!(
            kinds,
            [equality, None, equality, None, equality, equality, None]
        );
    }

    #[test]
    fn rejects_arrays_of_unsupported_elements() {
        let parsed = parse("CREATE TABLE hosts (addresses INET[] NOT NULL);");
        assert_eq!(parsed.schema.tables.len(), 0);
        assert!(parsed.failures[0].reason.contains("not supported"));
    }

    #[test]
    fn primary_key_implies_not_null() {
        let table = one_table("CREATE TABLE users (id UUID PRIMARY KEY, email TEXT NOT NULL);");
        assert!(!table.columns[0].nullable);
        assert_eq!(table.columns[0].rust_type, "uuid::Uuid");
    }

    #[test]
    fn stores_single_column_foreign_keys() {
        let table = one_table(
            "CREATE TABLE posts (id BIGSERIAL PRIMARY KEY, author_id UUID REFERENCES users (id));",
        );
        assert_eq!(table.foreign_keys.len(), 1);
        assert_eq!(table.foreign_keys[0].column, "author_id");
        assert_eq!(table.foreign_keys[0].referenced_table, "users");
        assert_eq!(table.foreign_keys[0].referenced_column, "id");
    }

    #[test]
    fn stores_table_level_foreign_keys() {
        let table = one_table(
            "CREATE TABLE posts (id BIGSERIAL PRIMARY KEY, author_id UUID, FOREIGN KEY (author_id) REFERENCES users (id));",
        );
        assert_eq!(table.foreign_keys.len(), 1);
        assert_eq!(table.foreign_keys[0].referenced_schema, None);
        assert_eq!(table.foreign_keys[0].referenced_table, "users");
    }

    #[test]
    fn foreign_keys_keep_the_referenced_schema() {
        let table = one_table(
            "CREATE TABLE posts (
                id BIGSERIAL PRIMARY KEY,
                author_id UUID REFERENCES core.users (id),
                editor_id UUID,
                FOREIGN KEY (editor_id) REFERENCES \"Audit\".users (id)
            );",
        );
        let schemas: Vec<Option<&str>> = table
            .foreign_keys
            .iter()
            .map(|key| key.referenced_schema.as_deref())
            .collect();
        assert_eq!(schemas, [Some("core"), Some("Audit")]);
    }

    #[test]
    fn renames_keyword_columns() {
        let table = one_table("CREATE TABLE articles (id BIGSERIAL PRIMARY KEY, \"type\" TEXT);");
        assert_eq!(table.columns[1].rust_name, "type_");
        assert_eq!(table.columns[1].sql_name, "type");
    }

    #[test]
    fn unqualified_tables_have_no_schema() {
        let table = one_table("CREATE TABLE users (id BIGSERIAL PRIMARY KEY);");
        assert_eq!(table.schema, None);
        assert_eq!(table.display_name(), "users");
    }

    #[test]
    fn keeps_quoted_schema_case() {
        let table = one_table("CREATE TABLE \"Sales\".orders (id BIGSERIAL PRIMARY KEY);");
        assert_eq!(table.schema.as_deref(), Some("Sales"));
    }

    #[test]
    fn maps_unusual_identifiers_to_valid_rust_names() {
        let table = one_table(
            "CREATE TABLE \"Reserved Words\" (\"select\" BIGSERIAL PRIMARY KEY, \"1st\" BOOLEAN, \"Mixed Case\" INT);",
        );
        assert_eq!(table.module_name, "reserved_words");
        assert_eq!(table.struct_name, "ReservedWord");
        assert_eq!(table.columns[0].rust_name, "select");
        assert_eq!(table.columns[1].rust_name, "_1st");
        assert_eq!(table.columns[1].sql_name, "1st");
        assert_eq!(table.columns[2].rust_name, "mixed_case");
    }

    #[test]
    fn rejects_columns_without_a_rust_identifier() {
        let parsed = parse("CREATE TABLE odd (id BIGSERIAL PRIMARY KEY, \"%\" TEXT);");
        assert_eq!(parsed.schema.tables.len(), 0);
        assert!(parsed.failures[0].reason.contains("column `%`"));
        assert!(parsed.failures[0].reason.contains("Rust identifier"));
    }

    #[test]
    fn detects_rust_field_collisions() {
        let parsed = parse("CREATE TABLE odd (id BIGSERIAL PRIMARY KEY, \"Foo\" TEXT, foo TEXT);");
        assert_eq!(parsed.schema.tables.len(), 0);
        assert!(parsed.failures[0].reason.contains("already taken"));
    }

    #[test]
    fn collects_failures_per_table() {
        let parsed =
            parse("CREATE TABLE good (id BIGSERIAL PRIMARY KEY); CREATE TABLE bad (x INET);");
        assert_eq!(parsed.schema.tables.len(), 1);
        assert_eq!(parsed.failures.len(), 1);
        assert_eq!(parsed.failures[0].table, "bad");
    }

    #[test]
    fn rejects_tables_named_after_generated_modules() {
        let parsed = parse(
            "CREATE TABLE search (id BIGSERIAL PRIMARY KEY); CREATE TABLE schema (id BIGSERIAL PRIMARY KEY); CREATE TABLE session (id BIGSERIAL PRIMARY KEY); CREATE TABLE nullable (id BIGSERIAL PRIMARY KEY);",
        );
        assert!(parsed.schema.tables.is_empty());
        assert_eq!(parsed.failures.len(), 4);
        assert!(
            parsed.failures[3]
                .reason
                .contains("reserved for the generated `db/nullable.rs`")
        );
        assert!(
            parsed.failures[0]
                .reason
                .contains("reserved for the generated `db/search.rs`")
        );
        assert!(
            parsed.failures[2]
                .reason
                .contains("reserved for the generated `db/session.rs`")
        );
    }

    #[test]
    fn rejects_module_name_collisions() {
        let parsed = parse(
            "CREATE TABLE users (id BIGSERIAL PRIMARY KEY); CREATE TABLE \"Users\" (id BIGSERIAL PRIMARY KEY);",
        );
        assert_eq!(parsed.schema.tables.len(), 1);
        assert_eq!(parsed.failures.len(), 1);
    }

    #[test]
    fn ignores_non_create_table_statements() {
        let table = one_table(
            "CREATE INDEX idx ON users (id); CREATE TABLE users (id BIGSERIAL PRIMARY KEY);",
        );
        assert_eq!(table.sql_name, "users");
    }

    #[test]
    fn reports_empty_schema() {
        assert!(parse_schema("SELECT 1;").is_err());
    }

    fn parse_files(files: &[(&str, &str)]) -> ParsedSchema {
        let sources: Vec<SchemaSource<'_>> = files
            .iter()
            .map(|(path, sql)| SchemaSource {
                path: Path::new(path),
                sql,
            })
            .collect();
        parse_sources(&sources).expect("schema should parse")
    }

    fn column_names(table: &Table) -> Vec<&str> {
        table
            .columns
            .iter()
            .map(|column| column.sql_name.as_str())
            .collect()
    }

    #[test]
    fn applies_column_changes_from_later_migrations() {
        let parsed = parse_files(&[
            (
                "0001_create.up.sql",
                "CREATE TABLE notes (id BIGSERIAL PRIMARY KEY, title TEXT NOT NULL, legacy TEXT);",
            ),
            (
                "0002_alter.up.sql",
                "ALTER TABLE notes ADD COLUMN body TEXT NOT NULL DEFAULT '';
                 ALTER TABLE notes RENAME COLUMN title TO headline;
                 ALTER TABLE notes DROP COLUMN legacy;",
            ),
        ]);
        assert!(parsed.failures.is_empty(), "{:?}", parsed.failures);
        let table = &parsed.schema.tables[0];
        assert_eq!(column_names(table), ["id", "headline", "body"]);
        assert_eq!(table.columns[1].rust_name, "headline");
        assert_eq!(table.columns[1].rust_type, "String");
        assert_eq!(table.columns[2].rust_type, "String");
        assert!(table.columns[2].has_default);
    }

    #[test]
    fn records_constraints_added_by_alter_table_like_inline_ones() {
        let table = one_table(
            "CREATE TABLE app.posts (id UUID NOT NULL, author_id UUID, slug TEXT NOT NULL);
             ALTER TABLE app.posts ADD CONSTRAINT posts_pk PRIMARY KEY (id);
             ALTER TABLE app.posts ADD CONSTRAINT posts_author_fk
               FOREIGN KEY (author_id) REFERENCES app.users (id);
             ALTER TABLE app.posts ADD CONSTRAINT posts_slug_key UNIQUE (slug);",
        );
        assert_eq!(table.primary_key, ["id"]);
        assert!(table.columns[0].is_primary_key);
        assert_eq!(table.foreign_keys.len(), 1);
        assert_eq!(table.foreign_keys[0].column, "author_id");
        assert_eq!(
            table.foreign_keys[0].referenced_schema.as_deref(),
            Some("app")
        );
        assert_eq!(table.foreign_keys[0].referenced_table, "users");
        assert_eq!(table.foreign_keys[0].referenced_column, "id");
        assert_eq!(table.unique_keys, [UniqueKey::new(vec!["slug".to_owned()])]);
    }

    #[test]
    fn primary_key_added_later_makes_its_column_required() {
        let table = one_table(
            "CREATE TABLE tags (code TEXT, label TEXT);
             ALTER TABLE tags ADD PRIMARY KEY (code);",
        );
        assert!(!table.columns[0].nullable);
        assert_eq!(table.columns[0].rust_type, "String");
        assert_eq!(table.columns[1].rust_type, "Option<String>");
    }

    #[test]
    fn renaming_a_key_column_renames_it_in_every_key() {
        let table = one_table(
            "CREATE TABLE posts (id BIGSERIAL PRIMARY KEY, author_id UUID REFERENCES users (id) UNIQUE);
             ALTER TABLE posts RENAME COLUMN id TO post_id;
             ALTER TABLE posts RENAME COLUMN author_id TO writer_id;",
        );
        assert_eq!(table.primary_key, ["post_id"]);
        assert_eq!(table.foreign_keys[0].column, "writer_id");
        assert_eq!(table.unique_keys[0].columns, ["writer_id"]);
    }

    #[test]
    fn dropping_a_key_column_drops_its_keys() {
        let table = one_table(
            "CREATE TABLE posts (id BIGINT PRIMARY KEY, author_id UUID REFERENCES users (id), title TEXT UNIQUE);
             ALTER TABLE posts DROP COLUMN id, DROP COLUMN author_id, DROP COLUMN title, ADD COLUMN body TEXT;",
        );
        assert!(table.primary_key.is_empty());
        assert!(table.foreign_keys.is_empty());
        assert!(table.unique_keys.is_empty());
        assert_eq!(column_names(&table), ["body"]);
    }

    #[test]
    fn ignores_statements_that_leave_the_model_unchanged() {
        let parsed = parse(
            "CREATE TABLE notes (id BIGSERIAL PRIMARY KEY, body TEXT);
             ALTER TABLE notes ENABLE ROW LEVEL SECURITY;
             CREATE POLICY notes_tenant ON notes USING (true);
             CREATE INDEX notes_body_index ON notes (body);
             GRANT SELECT ON notes TO reader;
             CREATE FUNCTION touch() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RETURN NEW; END $$;
             CREATE TRIGGER notes_touch BEFORE UPDATE ON notes FOR EACH ROW EXECUTE FUNCTION touch();
             COMMENT ON TABLE notes IS 'Notes';
             SELECT prepare_table('notes');",
        );
        assert!(parsed.failures.is_empty(), "{:?}", parsed.failures);
        let table = &parsed.schema.tables[0];
        assert_eq!(column_names(table), ["id", "body"]);
        assert!(table.unique_keys.is_empty());
    }

    #[test]
    fn rejects_unsupported_alter_table_on_a_known_table_with_its_location() {
        let parsed = parse_files(&[
            (
                "migrations/0001_create.up.sql",
                "CREATE TABLE prices (id BIGSERIAL PRIMARY KEY, amount INTEGER NOT NULL);",
            ),
            (
                "migrations/0002_rename.up.sql",
                "-- rename prices\n\nALTER TABLE prices RENAME TO costs;",
            ),
        ]);
        assert!(parsed.schema.tables.is_empty());
        assert_eq!(parsed.failures.len(), 1);
        let message = parsed.failures[0].to_string();
        assert!(
            message.starts_with(&format!(
                "{}:3: table `prices`: unsupported ALTER TABLE clause `RENAME",
                Path::new("migrations/0002_rename.up.sql").display()
            )),
            "{message}"
        );
    }

    /// Like a `CHECK` in `CREATE TABLE`, one added later changes the model
    /// only when it is a value list on a text column.
    #[test]
    fn ignores_checks_added_later_that_are_not_value_lists() {
        let parsed = parse(
            "CREATE TABLE prices (id BIGSERIAL PRIMARY KEY, amount INTEGER NOT NULL, code TEXT);
             ALTER TABLE prices ADD CONSTRAINT positive CHECK (amount > 0);
             ALTER TABLE prices ADD CHECK (amount IN (1, 2)), ADD CHECK (code <> '');
             ALTER TABLE prices ADD CHECK (id IN ('1'));",
        );
        assert!(parsed.failures.is_empty(), "{:?}", parsed.failures);
        let table = &parsed.schema.tables[0];
        assert!(
            table
                .columns
                .iter()
                .all(|column| column.value_list.is_none()),
            "{:?}",
            table.columns
        );
        assert!(parsed.schema.enums.is_empty());
    }

    #[test]
    fn rejects_alter_table_naming_a_missing_column() {
        let parsed = parse(
            "CREATE TABLE notes (id BIGSERIAL PRIMARY KEY);
             ALTER TABLE notes RENAME COLUMN title TO headline;",
        );
        assert!(parsed.schema.tables.is_empty());
        assert!(
            parsed.failures[0]
                .reason
                .contains("RENAME COLUMN: column `title` does not exist")
        );
    }

    #[test]
    fn ignores_alter_table_on_unknown_tables() {
        let table = one_table(
            "CREATE TABLE notes (id BIGSERIAL PRIMARY KEY);
             ALTER TABLE other.notes ALTER COLUMN id TYPE BIGINT;
             ALTER TABLE elsewhere ADD COLUMN x INET;",
        );
        assert_eq!(column_names(&table), ["id"]);
    }

    #[test]
    fn rejects_columns_added_with_unsupported_types() {
        let parsed = parse(
            "CREATE TABLE shifts (id BIGSERIAL PRIMARY KEY);
             ALTER TABLE public.shifts ADD COLUMN span INET;",
        );
        assert!(parsed.schema.tables.is_empty());
        assert_eq!(parsed.failures[0].line, Some(2));
        assert!(
            parsed.failures[0]
                .reason
                .contains("ADD COLUMN: column `span`: type `INET` is not supported yet"),
            "{:?}",
            parsed.failures
        );
    }

    /// Expected names are the ones PostgreSQL 17 chose for the same DDL.
    #[test]
    fn key_constraints_get_postgres_default_names() {
        let names = |sql: &str| -> Vec<String> {
            one_table(sql)
                .key_constraints
                .into_iter()
                .map(|constraint| constraint.name)
                .collect()
        };
        assert_eq!(
            names(
                "CREATE TABLE t (id BIGINT PRIMARY KEY, a TEXT UNIQUE, b TEXT, c TEXT, d TEXT,
                     CONSTRAINT named_bc UNIQUE (b, c));
                 ALTER TABLE t ADD UNIQUE (c, d);
                 CREATE UNIQUE INDEX t_d_idx ON t (d);"
            ),
            ["t_pkey", "t_a_key", "named_bc", "t_c_d_key"]
        );
        assert_eq!(
            names(
                "CREATE TABLE averyveryveryveryveryverylongtablenamethatgoesonandonandon (
                     abcdefghijklmnopqrstuvwxyzabcdefghij TEXT UNIQUE, x TEXT)"
            ),
            ["averyveryveryveryveryverylong_abcdefghijklmnopqrstuvwxyzabc_key"]
        );
        assert_eq!(
            names(
                "CREATE TABLE u (a TEXT UNIQUE, b TEXT, UNIQUE (a)); ALTER TABLE u ADD UNIQUE (a)"
            ),
            ["u_a_key", "u_a_key1"]
        );
    }

    #[test]
    fn records_unique_indexes_on_plain_columns() {
        let table = one_table(
            "CREATE TABLE users (id BIGSERIAL PRIMARY KEY, email TEXT, deleted_at TIMESTAMPTZ);
             CREATE UNIQUE INDEX users_email_live ON users (email) WHERE deleted_at IS NULL;
             CREATE UNIQUE INDEX users_email_lower ON users (lower(email));
             CREATE INDEX users_deleted ON users (deleted_at);",
        );
        assert_eq!(
            table.unique_keys,
            [UniqueKey {
                columns: vec!["email".to_owned()],
                predicate: Some("deleted_at IS NULL".to_owned()),
            }]
        );
    }

    fn variant_names(schema: &Schema) -> Vec<Vec<(&str, &str)>> {
        schema
            .enums
            .iter()
            .map(|enum_type| {
                enum_type
                    .variants
                    .iter()
                    .map(|variant| (variant.label.as_str(), variant.rust_name.as_str()))
                    .collect()
            })
            .collect()
    }

    #[test]
    fn enum_columns_use_the_shared_types_module() {
        let parsed = parse(
            "CREATE TYPE mood AS ENUM ('Happy', 'SAD', 'so-so', 'Very High', 'self', '9 lives');
             CREATE TYPE app.\"Ticket State\" AS ENUM ('open');
             CREATE TABLE people (
                 id BIGSERIAL PRIMARY KEY,
                 mood MOOD NOT NULL,
                 state app.\"Ticket State\",
                 history public.mood[]
             );",
        );
        assert!(parsed.failures.is_empty(), "{:?}", parsed.failures);
        let enums = &parsed.schema.enums;
        assert_eq!(enums[0].schema, None);
        assert_eq!(enums[0].sql_name, "mood");
        assert_eq!(enums[0].rust_name, "Mood");
        assert_eq!(enums[1].schema.as_deref(), Some("app"));
        assert_eq!(enums[1].sql_name, "Ticket State");
        assert_eq!(enums[1].rust_name, "TicketState");
        assert_eq!(
            variant_names(&parsed.schema)[0],
            [
                ("Happy", "Happy"),
                ("SAD", "Sad"),
                ("so-so", "SoSo"),
                ("Very High", "VeryHigh"),
                ("self", "Self_"),
                ("9 lives", "V9Lives"),
            ]
        );

        let columns = &parsed.schema.tables[0].columns;
        assert_eq!(columns[1].rust_type, "types::Mood");
        assert_eq!(
            columns[1].filter_kind(),
            Some(crate::schema::FilterKind::Equality)
        );
        assert_eq!(columns[2].rust_type, "Option<types::TicketState>");
        assert_eq!(columns[2].sqlx_type, "types::TicketState");
        assert_eq!(columns[3].rust_type, "Option<Vec<types::Mood>>");
        assert_eq!(columns[3].filter_kind(), None);
    }

    #[test]
    fn enums_in_different_schemas_keep_apart() {
        let parsed = parse(
            "CREATE TYPE status AS ENUM ('a');
             CREATE TYPE billing.status AS ENUM ('b');
             CREATE TABLE invoices (
                 id BIGSERIAL PRIMARY KEY,
                 local STATUS NOT NULL,
                 billed billing.status NOT NULL
             );",
        );
        assert!(parsed.failures.is_empty(), "{:?}", parsed.failures);
        let names: Vec<&str> = parsed
            .schema
            .enums
            .iter()
            .map(|enum_type| enum_type.rust_name.as_str())
            .collect();
        assert_eq!(names, ["Status", "BillingStatus"]);
        let columns = &parsed.schema.tables[0].columns;
        assert_eq!(columns[1].rust_type, "types::Status");
        assert_eq!(columns[2].rust_type, "types::BillingStatus");
    }

    #[test]
    fn alter_type_adds_and_renames_values() {
        let parsed = parse(
            "CREATE TYPE app.state AS ENUM ('open', 'closed');
             CREATE TABLE tickets (id BIGSERIAL PRIMARY KEY, state app.state NOT NULL);
             ALTER TYPE app.state ADD VALUE 'Archived';
             ALTER TYPE app.state ADD VALUE 'draft' BEFORE 'open';
             ALTER TYPE app.state ADD VALUE IF NOT EXISTS 'Reopened' AFTER 'closed';
             ALTER TYPE app.state ADD VALUE IF NOT EXISTS 'open';
             ALTER TYPE app.state RENAME VALUE 'closed' TO 'CLOSED';
             ALTER TYPE unknown_type ADD VALUE 'x';",
        );
        assert!(parsed.failures.is_empty(), "{:?}", parsed.failures);
        assert_eq!(
            variant_names(&parsed.schema)[0],
            [
                ("draft", "Draft"),
                ("open", "Open"),
                ("CLOSED", "Closed"),
                ("Reopened", "Reopened"),
                ("Archived", "Archived"),
            ]
        );
    }

    #[test]
    fn drop_type_removes_the_enum_and_cascades_to_its_columns() {
        let parsed = parse(
            "CREATE TYPE app.state AS ENUM ('open', 'closed');
             CREATE TYPE mood AS ENUM ('ok');
             CREATE TABLE tickets (id BIGSERIAL PRIMARY KEY, state app.state NOT NULL, title TEXT);
             CREATE TABLE notes (id BIGSERIAL PRIMARY KEY, mood mood);
             DROP TYPE IF EXISTS app.state, missing CASCADE;
             DROP TYPE mood;",
        );
        let names: Vec<&str> = parsed
            .schema
            .enums
            .iter()
            .map(|enum_type| enum_type.sql_name.as_str())
            .collect();
        assert!(names.is_empty(), "{names:?}");
        let tickets = &parsed.schema.tables[0];
        assert_eq!(column_names(tickets), ["id", "title"]);
        assert_eq!(parsed.failures.len(), 1, "{:?}", parsed.failures);
        assert_eq!(parsed.failures[0].table, "notes");
        assert_eq!(parsed.failures[0].line, Some(6));
        assert!(
            parsed.failures[0].reason.contains("DROP TYPE `mood`"),
            "{}",
            parsed.failures[0].reason
        );
    }

    #[test]
    fn drop_table_forgets_a_table_that_failed_earlier() {
        let parsed = parse(
            "CREATE TABLE users (id BIGSERIAL PRIMARY KEY);
             CREATE TABLE app.spans (id BIGSERIAL PRIMARY KEY, length INET);
             CREATE TABLE spans (id BIGSERIAL PRIMARY KEY, length INET);
             DROP TABLE app.spans;",
        );
        assert_eq!(parsed.failures.len(), 1, "{:?}", parsed.failures);
        assert_eq!(parsed.failures[0].line, Some(3));
    }

    #[test]
    fn enum_types_that_cannot_be_generated_fail_their_tables() {
        let parsed = parse(
            "CREATE TYPE clash AS ENUM ('in progress', 'IN_PROGRESS');
             CREATE TYPE blank AS ENUM ('!!');
             CREATE TYPE fine AS ENUM ('a', 'b');
             CREATE TABLE a (id BIGSERIAL PRIMARY KEY, x clash);
             CREATE TABLE b (id BIGSERIAL PRIMARY KEY, x blank[]);
             CREATE TABLE c (id BIGSERIAL PRIMARY KEY, x fine);
             CREATE TABLE d (id BIGSERIAL PRIMARY KEY);
             ALTER TYPE fine ADD VALUE 'a';
             ALTER TYPE fine RENAME TO finer;",
        );
        let reasons: Vec<(&str, &str)> = parsed
            .failures
            .iter()
            .map(|failure| (failure.table.as_str(), failure.reason.as_str()))
            .collect();
        assert_eq!(
            reasons,
            [
                (
                    "a",
                    "column `x`: type `clash` cannot be generated: labels `in progress` and `IN_PROGRESS` both map to the variant `InProgress`"
                ),
                (
                    "b",
                    "column `x`: type `blank` cannot be generated: label `!!` has no characters usable in a Rust identifier"
                ),
                (
                    "c",
                    "ALTER TYPE `fine`: ADD VALUE: label `a` already exists"
                ),
            ]
        );
        assert_eq!(parsed.failures[2].line, Some(8));
        let tables: Vec<&str> = parsed
            .schema
            .tables
            .iter()
            .map(|table| table.sql_name.as_str())
            .collect();
        assert_eq!(tables, ["d"]);
        assert!(
            parsed.schema.enums.is_empty(),
            "only generatable enums are generated"
        );
    }

    #[test]
    fn alter_type_rename_renames_the_enum_and_its_columns_follow() {
        let parsed = parse(
            "CREATE TYPE mood AS ENUM ('a');
             CREATE TABLE people (id BIGSERIAL PRIMARY KEY, mood mood NOT NULL, past mood[], next mood);
             ALTER TYPE mood RENAME TO feeling;
             CREATE TABLE later (id BIGSERIAL PRIMARY KEY, feeling feeling NOT NULL);",
        );
        assert!(parsed.failures.is_empty(), "{:?}", parsed.failures);
        let feeling = &parsed.schema.enums[0];
        assert_eq!(
            (feeling.sql_name.as_str(), feeling.rust_name.as_str()),
            ("feeling", "Feeling")
        );
        let types: Vec<(&str, &str)> = parsed
            .schema
            .tables
            .iter()
            .flat_map(|table| &table.columns[1..])
            .map(|column| (column.sqlx_type.as_str(), column.rust_type.as_str()))
            .collect();
        assert_eq!(
            types,
            [
                ("types::Feeling", "types::Feeling"),
                ("Vec<types::Feeling>", "Option<Vec<types::Feeling>>"),
                ("types::Feeling", "Option<types::Feeling>"),
                ("types::Feeling", "types::Feeling"),
            ]
        );
    }

    #[test]
    fn alter_type_rename_keeps_the_schema_and_avoids_taken_rust_names() {
        let parsed = parse(
            "CREATE TYPE status AS ENUM ('a');
             CREATE TYPE app.mood AS ENUM ('b');
             CREATE TABLE app.people (id BIGSERIAL PRIMARY KEY, mood app.mood);
             ALTER TYPE app.mood RENAME TO status;",
        );
        assert!(parsed.failures.is_empty(), "{:?}", parsed.failures);
        let renamed = &parsed.schema.enums[1];
        assert_eq!(renamed.display_name(), "app.status");
        assert_eq!(renamed.rust_name, "AppStatus");
        assert_eq!(
            parsed.schema.tables[0].columns[1].sqlx_type,
            "types::AppStatus"
        );
    }

    #[test]
    fn alter_type_rename_to_a_taken_name_fails_tables_using_the_type() {
        let parsed = parse(
            "CREATE TYPE mood AS ENUM ('a');
             CREATE TYPE feeling AS ENUM ('b');
             CREATE TABLE people (id BIGSERIAL PRIMARY KEY, mood mood);
             ALTER TYPE mood RENAME TO feeling;",
        );
        assert!(parsed.schema.tables.is_empty());
        assert_eq!(
            parsed.failures[0].reason,
            "ALTER TYPE `mood`: RENAME TO: type `feeling` already exists"
        );
    }

    #[test]
    fn rejects_tables_named_types() {
        let parsed = parse("CREATE TABLE types (id BIGSERIAL PRIMARY KEY);");
        assert!(
            parsed.failures[0]
                .reason
                .contains("reserved for the generated `db/types.rs`")
        );
    }

    fn enum_summary(schema: &Schema) -> Vec<(String, Option<&str>, Vec<&str>)> {
        schema
            .enums
            .iter()
            .map(|enum_type| {
                (
                    enum_type.rust_name.clone(),
                    enum_type.column.as_deref(),
                    enum_type
                        .variants
                        .iter()
                        .map(|variant| variant.label.as_str())
                        .collect(),
                )
            })
            .collect()
    }

    #[test]
    fn value_list_columns_become_enums_named_after_table_and_column() {
        let parsed = parse(
            "CREATE TABLE catalog.products (
                 id BIGSERIAL PRIMARY KEY,
                 product_type TEXT NOT NULL DEFAULT 'stocked'
                     CHECK (product_type IN ('stocked', 'Made To Order', 'stocked')),
                 measures VARCHAR(20),
                 type TEXT CHECK (type IN ('a', 'b')),
                 CONSTRAINT measures_known CHECK (measures IN ('count', 'weight'))
             );
             CREATE TABLE orders (
                 id BIGSERIAL PRIMARY KEY,
                 status TEXT NOT NULL CHECK (status IN ('open', 'closed'))
             );
             CREATE TABLE invoices (
                 id BIGSERIAL PRIMARY KEY,
                 status TEXT NOT NULL CHECK (status IN ('draft', 'paid'))
             );",
        );
        assert!(parsed.failures.is_empty(), "{:?}", parsed.failures);
        assert_eq!(
            enum_summary(&parsed.schema),
            [
                (
                    "ProductProductType".to_owned(),
                    Some("catalog.products.product_type"),
                    vec!["stocked", "Made To Order"],
                ),
                (
                    "ProductMeasures".to_owned(),
                    Some("catalog.products.measures"),
                    vec!["count", "weight"],
                ),
                (
                    "ProductType".to_owned(),
                    Some("catalog.products.type"),
                    vec!["a", "b"],
                ),
                (
                    "OrderStatus".to_owned(),
                    Some("orders.status"),
                    vec!["open", "closed"],
                ),
                (
                    "InvoiceStatus".to_owned(),
                    Some("invoices.status"),
                    vec!["draft", "paid"],
                ),
            ]
        );
        let products = &parsed.schema.tables[0].columns;
        assert_eq!(products[1].rust_type, "types::ProductProductType");
        assert_eq!(products[1].sqlx_type, "types::ProductProductType");
        assert!(products[1].has_default);
        assert_eq!(products[2].rust_type, "Option<types::ProductMeasures>");
        assert_eq!(
            products[2].filter_kind(),
            Some(crate::schema::FilterKind::Equality)
        );
        assert_eq!(products[3].rust_type, "Option<types::ProductType>");
        assert_eq!(
            parsed.schema.tables[2].columns[1].rust_type,
            "types::InvoiceStatus"
        );
    }

    #[test]
    fn checks_of_any_other_shape_leave_columns_as_they_are() {
        let table = one_table(
            "CREATE TABLE things (
                 id BIGSERIAL PRIMARY KEY,
                 negated TEXT CHECK (negated NOT IN ('a')),
                 numbers INTEGER CHECK (numbers IN (1, 2)),
                 mixed TEXT CHECK (mixed IN ('a', 1)),
                 combined TEXT CHECK (combined IN ('a') OR combined IS NULL),
                 pattern TEXT CHECK (pattern ~ '^[a-z]+$'),
                 tags TEXT[] CHECK (tags IN ('a')),
                 CHECK (length(pattern) > 1)
             );",
        );
        let types: Vec<&str> = table
            .columns
            .iter()
            .map(|column| column.rust_type.as_str())
            .collect();
        assert_eq!(
            types,
            [
                "i64",
                "Option<String>",
                "Option<i32>",
                "Option<String>",
                "Option<String>",
                "Option<String>",
                "Option<Vec<String>>",
            ]
        );
    }

    #[test]
    fn value_lists_follow_alter_table() {
        let parsed = parse_files(&[
            (
                "0001.up.sql",
                "CREATE TABLE orders (id BIGSERIAL PRIMARY KEY, state TEXT NOT NULL);
                 ALTER TABLE orders ADD CONSTRAINT state_known CHECK (state IN ('new', 'done'));
                 ALTER TABLE orders ADD COLUMN channel TEXT CHECK (channel IN ('web', 'shop'));",
            ),
            (
                "0002.up.sql",
                "ALTER TABLE orders RENAME COLUMN state TO status;",
            ),
        ]);
        assert!(parsed.failures.is_empty(), "{:?}", parsed.failures);
        assert_eq!(
            enum_summary(&parsed.schema),
            [
                (
                    "OrderStatus".to_owned(),
                    Some("orders.status"),
                    vec!["new", "done"],
                ),
                (
                    "OrderChannel".to_owned(),
                    Some("orders.channel"),
                    vec!["web", "shop"],
                ),
            ]
        );
    }

    fn column<'a>(table: &'a Table, name: &str) -> &'a Column {
        table
            .columns
            .iter()
            .find(|column| column.sql_name == name)
            .unwrap_or_else(|| panic!("no column `{name}`"))
    }

    /// `TYPE`, with or without `USING`, gives the column its new type; keys
    /// name columns, so they follow the change.
    #[test]
    fn alter_column_type_changes_the_column_and_its_keys() {
        let parsed = parse_files(&[
            (
                "0001.up.sql",
                "CREATE TABLE readings (id INTEGER PRIMARY KEY, code VARCHAR(8) UNIQUE, amount INTEGER NOT NULL, note TEXT);",
            ),
            (
                "0002.up.sql",
                "ALTER TABLE readings ALTER COLUMN id TYPE BIGINT, ALTER COLUMN code SET DATA TYPE UUID USING code::uuid;
                 ALTER TABLE readings ALTER amount TYPE NUMERIC(12, 2) USING amount::numeric;
                 ALTER TABLE readings ALTER COLUMN note TYPE BYTEA USING convert_to(note, 'UTF8');",
            ),
        ]);
        assert!(parsed.failures.is_empty(), "{:?}", parsed.failures);
        let table = &parsed.schema.tables[0];
        let types: Vec<(&str, &str, bool)> = table
            .columns
            .iter()
            .map(|column| {
                (
                    column.rust_type.as_str(),
                    column.sqlx_type.as_str(),
                    column.nullable,
                )
            })
            .collect();
        assert_eq!(
            types,
            [
                ("i64", "i64", false),
                ("Option<uuid::Uuid>", "uuid::Uuid", true),
                ("rust_decimal::Decimal", "rust_decimal::Decimal", false),
                ("Option<Vec<u8>>", "Vec<u8>", true),
            ]
        );
        assert_eq!(table.primary_key, ["id"]);
        assert!(column(table, "id").is_primary_key);
        assert_eq!(table.unique_keys, [UniqueKey::new(vec!["code".to_owned()])]);
    }

    /// `(Rust type, nullable, has a default, generated)` of each column.
    fn shapes(table: &Table) -> Vec<(&str, &str, bool, bool, bool)> {
        table
            .columns
            .iter()
            .map(|column| {
                (
                    column.sql_name.as_str(),
                    column.rust_type.as_str(),
                    column.nullable,
                    column.has_default,
                    column.is_generated,
                )
            })
            .collect()
    }

    #[test]
    fn alter_column_sets_and_drops_not_null_and_defaults() {
        let table = one_table(
            "CREATE TABLE notes (
                 id BIGINT PRIMARY KEY,
                 title TEXT,
                 body TEXT NOT NULL DEFAULT '',
                 views INTEGER,
                 seq SERIAL NOT NULL
             );
             ALTER TABLE notes ALTER COLUMN title SET NOT NULL, ALTER COLUMN title TYPE VARCHAR(80);
             ALTER TABLE notes ALTER COLUMN body DROP NOT NULL, ALTER COLUMN body DROP DEFAULT;
             ALTER TABLE notes ALTER COLUMN views SET DEFAULT 0;
             ALTER TABLE notes ALTER COLUMN seq DROP DEFAULT;",
        );
        assert_eq!(
            shapes(&table),
            [
                ("id", "i64", false, false, false),
                ("title", "String", false, false, false),
                ("body", "Option<String>", true, false, false),
                ("views", "Option<i32>", true, true, false),
                ("seq", "i32", false, false, false),
            ]
        );
    }

    #[test]
    fn alter_column_add_identity_makes_the_column_generated() {
        let table = one_table(
            "CREATE TABLE notes (id BIGINT NOT NULL, other BIGINT NOT NULL);
             ALTER TABLE notes ALTER COLUMN id ADD GENERATED ALWAYS AS IDENTITY;
             ALTER TABLE notes ALTER COLUMN other ADD GENERATED BY DEFAULT AS IDENTITY (START WITH 10);
             ALTER TABLE notes ADD PRIMARY KEY (id);",
        );
        assert_eq!(
            shapes(&table),
            [
                ("id", "i64", false, true, true),
                ("other", "i64", false, true, true),
            ]
        );
    }

    /// A value list restricts text; a type change away from text drops it.
    #[test]
    fn alter_column_type_keeps_value_lists_only_on_text() {
        let parsed = parse(
            "CREATE TABLE orders (
                 id BIGSERIAL PRIMARY KEY,
                 state TEXT CHECK (state IN ('new', 'done')),
                 grade TEXT CHECK (grade IN ('1', '2'))
             );
             ALTER TABLE orders ALTER COLUMN state TYPE VARCHAR(10);
             ALTER TABLE orders ALTER COLUMN grade TYPE INTEGER USING grade::integer;",
        );
        assert!(parsed.failures.is_empty(), "{:?}", parsed.failures);
        assert_eq!(
            enum_summary(&parsed.schema),
            [(
                "OrderState".to_owned(),
                Some("orders.state"),
                vec!["new", "done"]
            )]
        );
        let table = &parsed.schema.tables[0];
        assert_eq!(column(table, "grade").rust_type, "Option<i32>");
        assert_eq!(column(table, "grade").value_list, None);
    }

    #[test]
    fn rejects_alter_column_changes_it_cannot_apply_with_their_location() {
        let cases = [
            (
                "ALTER TABLE notes ALTER COLUMN body TYPE INET;",
                "ALTER COLUMN: column `body`: type `INET` is not supported yet",
            ),
            (
                "ALTER TABLE notes ALTER COLUMN body TYPE BIGSERIAL;",
                "ALTER COLUMN: column `body`: type `BIGSERIAL` exists only in CREATE TABLE and ADD COLUMN",
            ),
            (
                "ALTER TABLE notes ALTER COLUMN id DROP NOT NULL;",
                "ALTER COLUMN: column `id` is in the primary key, so it cannot drop NOT NULL",
            ),
            (
                "ALTER TABLE notes ALTER COLUMN body ADD GENERATED ALWAYS AS IDENTITY;",
                "ALTER COLUMN: column `body` must be NOT NULL before it can become an identity column",
            ),
            (
                "ALTER TABLE notes ALTER COLUMN title SET NOT NULL;",
                "ALTER COLUMN: column `title` does not exist",
            ),
        ];
        for (alter, reason) in cases {
            let sql = format!("-- change notes\n{alter}");
            let parsed = parse_files(&[
                (
                    "0001.up.sql",
                    "CREATE TABLE notes (id BIGINT PRIMARY KEY, body TEXT);",
                ),
                ("0002.up.sql", &sql),
            ]);
            assert!(parsed.schema.tables.is_empty(), "{alter}");
            assert_eq!(parsed.failures.len(), 1, "{alter}");
            assert_eq!(parsed.failures[0].reason, reason, "{alter}");
            assert_eq!(parsed.failures[0].line, Some(2), "{alter}");
            assert_eq!(
                parsed.failures[0].path.as_deref(),
                Some(Path::new("0002.up.sql")),
                "{alter}"
            );
        }
    }
    #[test]
    fn value_lists_that_cannot_be_generated_fail_their_tables() {
        let parsed = parse(
            "CREATE TYPE order_status AS ENUM ('a');
             CREATE TABLE clash (id BIGSERIAL PRIMARY KEY, x TEXT CHECK (x IN ('in progress', 'IN_PROGRESS')));
             CREATE TABLE blank (id BIGSERIAL PRIMARY KEY, x TEXT CHECK (x IN ('!!')));
             CREATE TABLE orders (id BIGSERIAL PRIMARY KEY, status TEXT CHECK (status IN ('a')));
             CREATE TABLE fine (id BIGSERIAL PRIMARY KEY, x TEXT CHECK (x IN ('a')));",
        );
        let reasons: Vec<(&str, &str)> = parsed
            .failures
            .iter()
            .map(|failure| (failure.table.as_str(), failure.reason.as_str()))
            .collect();
        assert_eq!(
            reasons,
            [
                (
                    "clash",
                    "column `x`: value list cannot be generated: labels `in progress` and `IN_PROGRESS` both map to the variant `InProgress`"
                ),
                (
                    "blank",
                    "column `x`: value list cannot be generated: label `!!` has no characters usable in a Rust identifier"
                ),
                (
                    "orders",
                    "column `status`: value list cannot be generated: its Rust name `OrderStatus` is already used by another enum type"
                ),
            ]
        );
        assert_eq!(parsed.failures[0].line, Some(2));
        let names: Vec<String> = enum_summary(&parsed.schema)
            .into_iter()
            .map(|(name, ..)| name)
            .collect();
        assert_eq!(names, ["OrderStatus", "FineX"]);
    }
}
