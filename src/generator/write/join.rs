//! `attach`, `detach`, and `sync` on a many-to-many join table.

use crate::schema::{Column, Table};

use crate::generator::relationships::JoinPair;
use crate::generator::sql::{first_placeholder, ident, table_ref, tenant_condition};
use crate::generator::{
    StaticQuery, doc_name, keys, param_name, param_type, push_doc, push_fn_head,
};

/// The local naming a join table's restored rows, for `attach` and `sync`.
const RESTORED: &str = "rok_sqlc_restored";

/// `attach`, `detach`, and `sync` for a join table (ticket 23): they add,
/// remove, and replace the pairs of one row of the pair's `near` table.
/// Each runs one statement (ADR 0006), so `sync` replaces the set
/// atomically inside the caller's transaction. On a soft-delete join table,
/// `detach` soft-deletes and `attach` restores the soft-deleted row rather
/// than inserting another. `attach` and `sync` insert only the pair (and the
/// tenant), so they are generated only when every other insertable column
/// can be left out.
pub(super) fn render_join_writes(output: &mut String, table: &Table, pair: &JoinPair<'_>) {
    let joins = JoinSql::new(table, pair);
    let near_param = format!("{}: {}", param_name(pair.near), param_type(pair.near));
    let far_param = format!("{}: {}", param_name(pair.far), param_type(pair.far));
    let near = doc_name(&pair.near_relationship.target);
    let far = doc_name(&pair.far_relationship.target);
    let required: Vec<&Column> = table
        .insert_columns()
        .into_iter()
        .filter(|column| {
            column.sql_name != pair.near.sql_name
                && column.sql_name != pair.far.sql_name
                && !column.has_default
                && !column.nullable
        })
        .collect();

    if let Some(column) = required.first() {
        output.push_str(&format!(
            "\n// `{}` has no default, so attach and sync, which insert only the pair, are not\n// generated for this join table.\n",
            column.sql_name.replace(['\r', '\n'], " ")
        ));
    } else {
        output.push('\n');
        let mut doc = vec![
            format!("Pairs the {near} row with the {far} row, and returns whether the pair"),
            "is new: `false` when they are already paired.".to_owned(),
        ];
        if let Some(live) = &joins.soft_delete {
            doc.push(format!(
                "A soft-deleted pairing is restored by clearing `{live}`, rather than"
            ));
            doc.push("inserted again.".to_owned());
        }
        push_doc(
            output,
            0,
            &doc.iter().map(String::as_str).collect::<Vec<_>>(),
        );
        push_fn_head(
            output,
            "attach",
            &keys::scoped_params(table, &[near_param.clone(), far_param.clone()]),
            "Result<bool, sqlx::Error>",
        );
        joins.push_attach(output);
        output.push_str("}\n");
    }

    output.push('\n');
    let doc = if let Some(live) = &joins.soft_delete {
        vec![
            format!("Unpairs the {near} row from the {far} row by setting `{live}` to the"),
            "current time, and returns whether they were paired.".to_owned(),
        ]
    } else {
        vec![
            format!("Unpairs the {near} row from the {far} row, and returns whether they"),
            "were paired.".to_owned(),
        ]
    };
    push_doc(
        output,
        0,
        &doc.iter().map(String::as_str).collect::<Vec<_>>(),
    );
    push_fn_head(
        output,
        "detach",
        &keys::scoped_params(table, &[near_param.clone(), far_param]),
        "Result<bool, sqlx::Error>",
    );
    joins.push_detach(output);
    output.push_str("    Ok(result.rows_affected() > 0)\n}\n");

    if required.is_empty() {
        output.push('\n');
        let mut doc = vec![
            format!(
                "Replaces the {far} rows paired with the {near} row by `{}`, in one",
                joins.set
            ),
            "statement: pairs missing from it are removed, and new ones added.".to_owned(),
        ];
        if let Some(live) = &joins.soft_delete {
            doc.push(format!(
                "Removed pairings are soft-deleted (`{live}` set to the current time),"
            ));
            doc.push("and soft-deleted ones in the set are restored.".to_owned());
        }
        push_doc(
            output,
            0,
            &doc.iter().map(String::as_str).collect::<Vec<_>>(),
        );
        push_fn_head(
            output,
            "sync",
            &keys::scoped_params(
                table,
                &[
                    near_param,
                    format!("{}: &[{}]", joins.set, pair.far.value_type()),
                ],
            ),
            "Result<(), sqlx::Error>",
        );
        joins.push_sync(output);
        output.push_str("    Ok(())\n}\n");
    }
}

/// The SQL fragments of a join table's writes.
struct JoinSql<'a> {
    table: &'a Table,
    pair: &'a JoinPair<'a>,
    /// The tenant column, when the join table has one: `$1`.
    tenant: Option<&'a Column>,
    /// The soft-delete column (identifier), when the join table has one.
    soft_delete: Option<String>,
    /// The name of `sync`'s slice parameter, e.g. `permission_keys`.
    set: String,
}

impl<'a> JoinSql<'a> {
    fn new(table: &'a Table, pair: &'a JoinPair<'a>) -> Self {
        Self {
            table,
            pair,
            tenant: table.tenant(),
            soft_delete: table.soft_delete_column.as_deref().map(ident),
            set: format!("{}s", param_name(pair.far).trim_end_matches('_')),
        }
    }

    /// The placeholder of the `near` value; the `far` value (or set) follows.
    fn first(&self) -> usize {
        first_placeholder(self.table)
    }

    /// `[t = $1 AND ]near = $n` over `alias.` columns (none when empty).
    fn side(&self, alias: &str) -> String {
        let prefix = if alias.is_empty() {
            String::new()
        } else {
            format!("{alias}.")
        };
        tenant_condition(self.table, (!alias.is_empty()).then_some(alias))
            .into_iter()
            .chain([format!(
                "{prefix}{} = ${}",
                ident(&self.pair.near.sql_name),
                self.first()
            )])
            .collect::<Vec<_>>()
            .join(" AND ")
    }

    /// `side AND far = $m`.
    fn both(&self) -> String {
        format!(
            "{} AND {} = ${}",
            self.side(""),
            ident(&self.pair.far.sql_name),
            self.first() + 1
        )
    }

    fn far(&self) -> String {
        ident(&self.pair.far.sql_name)
    }

    /// `INSERT INTO t (tenant, near, far)`.
    fn insert_into(&self) -> String {
        let columns: Vec<String> = self
            .tenant
            .into_iter()
            .chain([self.pair.near, self.pair.far])
            .map(|column| ident(&column.sql_name))
            .collect();
        format!(
            "INSERT INTO {} ({})",
            table_ref(self.table),
            columns.join(", ")
        )
    }

    /// `$1, $2[, $3]`: the tenant and `near` values.
    fn side_values(&self) -> Vec<String> {
        (1..=self.first())
            .map(|index| format!("${index}"))
            .collect()
    }

    /// `ON CONFLICT (key)[ WHERE live] DO NOTHING`: the join pair's unique
    /// key, with its partial index's condition.
    fn on_conflict(&self) -> String {
        let key: Vec<String> = self.pair.key.iter().map(|column| ident(column)).collect();
        let condition = match (&self.soft_delete, self.pair.partial) {
            (Some(live), true) => format!(" WHERE {live} IS NULL"),
            _ => String::new(),
        };
        format!("ON CONFLICT ({}){condition} DO NOTHING", key.join(", "))
    }

    fn binds(&self, last: &str) -> Vec<String> {
        keys::tenant_bind(self.table)
            .into_iter()
            .chain([param_name(self.pair.near), last.to_owned()])
            .collect()
    }

    fn push_attach(&self, output: &mut String) {
        let mut values = self.side_values();
        values.push(format!("${}", self.first() + 1));
        let values = values.join(", ");
        let binds = self.binds(&param_name(self.pair.far));
        let Some(live) = &self.soft_delete else {
            StaticQuery {
                call: "sqlx::query",
                sql: vec![
                    self.insert_into(),
                    format!("VALUES ({values})"),
                    self.on_conflict(),
                ],
                binds,
                execute: "execute(executor)",
            }
            .push(output, 4, "let result = ", "?;");
            output.push_str("    Ok(result.rows_affected() > 0)\n");
            return;
        };
        // The restore picks one soft-deleted row, and only when no live
        // pairing exists; the insert runs only when nothing was restored.
        let table = table_ref(self.table);
        let both = self.both();
        StaticQuery {
            call: "sqlx::query_scalar::<_, bool>",
            sql: vec![
                format!("WITH {RESTORED} AS ("),
                format!("    UPDATE {table}"),
                format!("    SET {live} = NULL"),
                "    WHERE ctid = (".to_owned(),
                "        SELECT ctid".to_owned(),
                format!("        FROM {table}"),
                format!("        WHERE {both} AND {live} IS NOT NULL"),
                format!(
                    "            AND NOT EXISTS (SELECT 1 FROM {table} WHERE {both} AND {live} IS NULL)"
                ),
                format!("        ORDER BY {live} DESC"),
                "        LIMIT 1".to_owned(),
                "    )".to_owned(),
                "    RETURNING 1".to_owned(),
                "), rok_sqlc_inserted AS (".to_owned(),
                format!("    {}", self.insert_into()),
                format!("    SELECT {values}"),
                format!("    WHERE NOT EXISTS (SELECT 1 FROM {RESTORED})"),
                format!("    {}", self.on_conflict()),
                "    RETURNING 1".to_owned(),
                ")".to_owned(),
                format!(
                    "SELECT EXISTS (SELECT 1 FROM {RESTORED}) OR EXISTS (SELECT 1 FROM rok_sqlc_inserted)"
                ),
            ],
            binds,
            execute: "fetch_one(executor)",
        }
        .push(output, 4, "", "");
    }

    fn push_detach(&self, output: &mut String) {
        let table = table_ref(self.table);
        let sql = match &self.soft_delete {
            Some(live) => vec![
                format!("UPDATE {table}"),
                format!("SET {live} = now()"),
                format!("WHERE {} AND {live} IS NULL", self.both()),
            ],
            None => vec![
                format!("DELETE FROM {table}"),
                format!("WHERE {}", self.both()),
            ],
        };
        StaticQuery {
            call: "sqlx::query",
            sql,
            binds: self.binds(&param_name(self.pair.far)),
            execute: "execute(executor)",
        }
        .push(output, 4, "let result = ", "?;");
    }

    fn push_sync(&self, output: &mut String) {
        let table = table_ref(self.table);
        let far = self.far();
        let set = format!("${}", self.first() + 1);
        let side = self.side("");
        let mut values = self.side_values();
        values.push("rok_sqlc_key".to_owned());
        let insert = [
            self.insert_into(),
            format!("SELECT DISTINCT {}", values.join(", ")),
            format!("FROM UNNEST({set}) AS rok_sqlc_wanted (rok_sqlc_key)"),
        ];
        let sql = match &self.soft_delete {
            None => {
                let mut sql = vec![
                    "WITH rok_sqlc_removed AS (".to_owned(),
                    format!("    DELETE FROM {table}"),
                    format!("    WHERE {side} AND {far} <> ALL({set})"),
                    ")".to_owned(),
                ];
                sql.extend(insert);
                sql.push(self.on_conflict());
                sql
            }
            Some(live) => {
                // Pairs outside the set are soft-deleted; for each pair in it
                // with no live row, one soft-deleted row is restored; the rest
                // are inserted. The three touch different rows.
                let deleted = self.side("rok_sqlc_deleted");
                let current = self.side("rok_sqlc_live");
                let mut sql = vec![
                    "WITH rok_sqlc_removed AS (".to_owned(),
                    format!("    UPDATE {table}"),
                    format!("    SET {live} = now()"),
                    format!("    WHERE {side} AND {live} IS NULL AND {far} <> ALL({set})"),
                    format!("), {RESTORED} AS ("),
                    format!("    UPDATE {table}"),
                    format!("    SET {live} = NULL"),
                    "    WHERE ctid IN (".to_owned(),
                    format!(
                        "        SELECT DISTINCT ON (rok_sqlc_deleted.{far}) rok_sqlc_deleted.ctid"
                    ),
                    format!("        FROM {table} AS rok_sqlc_deleted"),
                    format!("        WHERE {deleted}"),
                    format!("            AND rok_sqlc_deleted.{live} IS NOT NULL"),
                    format!("            AND rok_sqlc_deleted.{far} = ANY({set})"),
                    "            AND NOT EXISTS (".to_owned(),
                    format!("                SELECT 1 FROM {table} AS rok_sqlc_live"),
                    format!("                WHERE {current}"),
                    format!("                    AND rok_sqlc_live.{far} = rok_sqlc_deleted.{far}"),
                    format!("                    AND rok_sqlc_live.{live} IS NULL"),
                    "            )".to_owned(),
                    format!(
                        "        ORDER BY rok_sqlc_deleted.{far}, rok_sqlc_deleted.{live} DESC"
                    ),
                    "    )".to_owned(),
                    format!("    RETURNING {far}"),
                    ")".to_owned(),
                ];
                sql.extend(insert);
                sql.push(format!(
                    "WHERE NOT EXISTS (SELECT 1 FROM {RESTORED} WHERE {RESTORED}.{far} = rok_sqlc_key)"
                ));
                sql.push(self.on_conflict());
                sql
            }
        };
        StaticQuery {
            call: "sqlx::query",
            sql,
            binds: self.binds(&self.set),
            execute: "execute(executor)",
        }
        .push(output, 4, "", "?;");
    }
}
