//! The statement loop over the schema inputs, and the recovery from
//! statements the SQL parser cannot read.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use sqlparser::ast::{
    AlterColumnOperation, AlterIndexOperation, AlterTableOperation, ObjectName, ObjectType,
    Spanned, Statement, UserDefinedTypeRepresentation,
};
use sqlparser::dialect::PostgreSqlDialect;
use sqlparser::keywords::Keyword;
use sqlparser::parser::Parser;
use sqlparser::tokenizer::{Token, Tokenizer};

use super::alter::{apply_alter, drop_table};
use super::constraints::record_unique_index;
use super::enums::{
    EnumRegistry, drop_columns_of_type, resolve_value_lists, retype_columns, uses_type,
};
use super::names::{
    OtherRelations, find_table, ident_name, object_leaf_name, object_schema_name, relation_names,
    rename_relation,
};
use super::recover::{
    AlterRewrite, Recovered, recover_statement, rewrite_alter_table, split_statements,
};
use super::table::{build_table, table_failure};
use super::triggers::{
    forget_trigger, has_before_insert_trigger, record_lost_triggers, record_trigger,
};
use super::{GENERATED_MODULES, ParsedSchema, SchemaSource, StatementLocation, names_table};
use crate::parser::diagnostics::{LastGood, TableFailure};
use crate::parser::naming;
use crate::report::display_path;
use crate::schema::{Schema, Table};

/// [`parse_sources`](super::parse_sources) with module renames (the `[modules]` configuration):
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
    let mut source_removed: HashMap<(String, Option<String>), StatementLocation> = HashMap::new();
    let mut order = 0;

    for source in sources {
        for statement in parse_statements(&dialect, source)? {
            order += 1;
            let statement = match statement {
                Parsed::Statement(statement) => *statement,
                Parsed::RenameRelation { name, new_name } => {
                    rename_relation(&mut tables, &mut others, &name, &new_name);
                    continue;
                }
                Parsed::UnreadableAlter(unparsed) => {
                    // An `ALTER TABLE` on an unknown table is ignored, like
                    // any other statement on one.
                    let Some(index) = find_table(&tables, &unparsed.table) else {
                        continue;
                    };
                    let table = tables.remove(index);
                    let mut failure = TableFailure {
                        path: Some(source.path.to_path_buf()),
                        line: Some(unparsed.line),
                        schema: table.schema.clone(),
                        ..table_failure(
                            &table.sql_name,
                            format!(
                                "the SQL parser cannot read this ALTER TABLE: {}",
                                unparsed.error
                            ),
                        )
                    };
                    failure.last_good = Some(LastGood::new(table));
                    failures.push(failure);
                    continue;
                }
            };
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
                            ..*failure
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
                    // A failing statement may have applied some clauses.
                    let before = tables[index].clone();
                    match apply_alter(&mut tables[index], &operations, &enums, &taken) {
                        Ok(()) => {
                            for operation in &operations {
                                if let AlterTableOperation::AlterColumn {
                                    column_name,
                                    op: AlterColumnOperation::DropDefault,
                                } = operation
                                {
                                    source_removed.insert(
                                        (
                                            tables[index].module_name.clone(),
                                            Some(ident_name(column_name)),
                                        ),
                                        StatementLocation {
                                            order,
                                            path: source.path.to_path_buf(),
                                            line,
                                        },
                                    );
                                }
                            }
                            continue;
                        }
                        Err(reason) => {
                            tables.remove(index);
                            TableFailure {
                                schema: before.schema.clone(),
                                last_good: Some(LastGood::new(before.clone())),
                                ..table_failure(&before.sql_name, reason)
                            }
                        }
                    }
                }
                Statement::AlterIndex {
                    name,
                    operation: AlterIndexOperation::RenameIndex { index_name },
                } => {
                    let new_name = object_leaf_name(&index_name);
                    rename_relation(&mut tables, &mut others, &name, &new_name);
                    continue;
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
                    let triggered = has_before_insert_trigger(&tables);
                    record_trigger(&mut tables, &trigger);
                    let here = StatementLocation {
                        order,
                        path: source.path.to_path_buf(),
                        line: line
                            .filter(|line| *line > 0)
                            .or_else(|| usize::try_from(trigger.name.span().start.line).ok()),
                    };
                    record_lost_triggers(&tables, &triggered, &mut source_removed, &here);
                    continue;
                }
                Statement::DropTrigger(trigger) => {
                    let triggered = has_before_insert_trigger(&tables);
                    forget_trigger(&mut tables, &trigger);
                    let here = StatementLocation {
                        order,
                        path: source.path.to_path_buf(),
                        // sqlparser gives `DROP TRIGGER` no span of its own; its name has one.
                        line: usize::try_from(trigger.trigger_name.span().start.line).ok(),
                    };
                    record_lost_triggers(&tables, &triggered, &mut source_removed, &here);
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
    let (value_lists, broken) = resolve_value_lists(&mut tables, &enums, &[]);
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
    // A failed table's last good shape resolves its value lists the same
    // way, after every table's that generates; one whose value lists cannot
    // be generated has no last good shape.
    let mut known = value_lists.clone();
    for failure in &mut failures {
        let Some(last_good) = &mut failure.last_good else {
            continue;
        };
        let (table_enums, broken) =
            resolve_value_lists(std::slice::from_mut(&mut last_good.table), &enums, &known);
        if broken.is_empty() {
            known.extend(table_enums.iter().cloned());
            last_good.enums = table_enums;
        } else {
            failure.last_good = None;
        }
    }

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
        source_removed,
        unmatched_modules: unmatched_modules.into_iter().cloned().collect(),
    })
}

/// An `ALTER TABLE` statement neither the SQL parser nor
/// [`rewrite_alter_table`] reads (such as `ALTER TABLE … SET SCHEMA`),
/// which fails the table it alters.
struct UnparsedAlter {
    /// The table the statement alters.
    table: ObjectName,
    /// 1-based line of the statement in its file.
    line: usize,
    /// What the SQL parser expected, without its location.
    error: String,
}

/// One statement of a schema input.
enum Parsed {
    Statement(Box<Statement>),
    UnreadableAlter(UnparsedAlter),
    /// A rename of an index, sequence, or view the SQL parser cannot read.
    RenameRelation {
        name: ObjectName,
        new_name: String,
    },
}

/// Parses one schema input into its statements. When the whole input does
/// not parse, it is split into statements (see [`split_statements`]) and
/// each is parsed on its own. A statement that does not parse is recognised
/// from its tokens where its effect on the model is known: an `ALTER TABLE`
/// is rewritten (see [`rewrite_alter_table`]) or comes back as an
/// [`UnparsedAlter`]; a function, procedure, `DO` block, or `ALTER` of an
/// index, sequence, or view is skipped or becomes a rename (see
/// [`recover_statement`]). Any other statement that does not parse fails the
/// run at its `path:line`. Spans keep their place in the whole input.
fn parse_statements(dialect: &PostgreSqlDialect, source: &SchemaSource<'_>) -> Result<Vec<Parsed>> {
    let invalid = |line: Option<u64>| {
        let place = match (source.path.as_os_str().is_empty(), line) {
            (true, None) => String::new(),
            (true, Some(line)) => format!("line {line}: "),
            (false, None) => format!("{}: ", display_path(source.path)),
            (false, Some(line)) => format!("{}:{line}: ", display_path(source.path)),
        };
        format!("{place}invalid PostgreSQL SQL syntax")
    };
    if let Ok(statements) = Parser::parse_sql(dialect, source.sql) {
        return Ok(statements
            .into_iter()
            .map(|statement| Parsed::Statement(Box::new(statement)))
            .collect());
    }
    let tokens = Tokenizer::new(dialect, source.sql)
        .tokenize_with_location()
        .with_context(|| invalid(None))?;
    let parse = |tokens: Vec<_>| {
        Parser::new(dialect)
            .with_tokens_with_locations(tokens)
            .parse_statements()
    };
    let mut statements = Vec::new();
    for chunk in split_statements(&tokens) {
        let Some(first) = chunk
            .iter()
            .find(|token| !matches!(token.token, Token::Whitespace(_) | Token::EOF))
        else {
            continue;
        };
        let line = first.span.start.line;
        let parsed =
            parse(chunk.to_vec()).or_else(|error| match rewrite_alter_table(dialect, chunk) {
                Some(AlterRewrite::Skip) => Ok(Vec::new()),
                Some(AlterRewrite::Tokens(tokens)) => parse(tokens).map_err(|_| error),
                None => Err(error),
            });
        match parsed {
            Ok(parsed) => statements.extend(
                parsed
                    .into_iter()
                    .map(|statement| Parsed::Statement(Box::new(statement))),
            ),
            Err(error) => {
                match recover_statement(dialect, chunk) {
                    Some(Recovered::Skip) => continue,
                    Some(Recovered::RenameRelation { name, new_name }) => {
                        statements.push(Parsed::RenameRelation { name, new_name });
                        continue;
                    }
                    None => {}
                }
                let mut parser = Parser::new(dialect).with_tokens_with_locations(chunk.to_vec());
                let table = parser
                    .parse_keywords(&[Keyword::ALTER, Keyword::TABLE])
                    .then(|| {
                        // Optional words: whether they are there does not matter.
                        let _ = parser.parse_keywords(&[Keyword::IF, Keyword::EXISTS]);
                        let _ = parser.parse_keyword(Keyword::ONLY);
                        parser.parse_object_name(false).ok()
                    })
                    .flatten();
                let Some(table) = table else {
                    return Err(anyhow::Error::new(error).context(invalid(Some(line))));
                };
                let error = error.to_string();
                let error = error
                    .split(" at Line: ")
                    .next()
                    .unwrap_or_default()
                    .trim_start_matches("sql parser error: ")
                    .to_owned();
                statements.push(Parsed::UnreadableAlter(UnparsedAlter {
                    table,
                    line: usize::try_from(line).unwrap_or_default(),
                    error,
                }));
            }
        }
    }
    Ok(statements)
}
