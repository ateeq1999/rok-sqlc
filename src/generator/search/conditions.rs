//! A filter's `push_conditions`: the column conditions, and the `EXISTS`
//! subqueries that follow relationships.

use crate::schema::{Relationship, Table, Through};

use super::filter::{ERRORS_DOC, push_column_conditions};
use super::{FilterRelationship, QUERY_PARAM, filter_relationships, push_call, push_call_ending};
use crate::generator::push_doc;
use crate::generator::sql::{ident, live_condition, str_literal, table_ref};

/// `push_conditions`: the column conditions, then an `EXISTS` subquery per
/// set relationship field. It takes the enclosing row's table or alias
/// (`outer`) and the nesting depth when the filter has relationship fields.
pub(super) fn push_conditions_fn(
    output: &mut String,
    table: &Table,
    tables: &[Table],
    relationships: &[FilterRelationship<'_>],
) {
    let mut doc = vec![
        "Appends ` AND <condition>` for each condition, without the `WHERE` clause or",
        "the soft-delete condition, so another resource's filter can nest this one.",
    ];
    if relationships.is_empty() {
        push_doc(output, 4, &doc);
        output.push_str(&format!(
            "    pub(crate) fn push_conditions(self, {QUERY_PARAM}) {{\n"
        ));
        push_column_conditions(output, table);
        output.push_str("    }\n");
        return;
    }
    doc.extend([
        "`outer` names the row being filtered (its table, or a relationship alias)",
        "and `depth` counts the relationship filters this one is nested in.",
    ]);
    doc.extend(ERRORS_DOC);
    push_doc(output, 4, &doc);
    output.push_str(&format!(
        "    pub(crate) fn push_conditions(\n        self,\n        outer: &str,\n        depth: usize,\n        {QUERY_PARAM},\n    ) -> Result<(), sqlx::Error> {{\n"
    ));
    push_column_conditions(output, table);
    let quoted = |name: &str| str_literal(&ident(name));
    for FilterRelationship {
        relationship,
        target,
        join,
    } in relationships
    {
        output.push_str(&format!(
            "        if let Some(filter) = self.{} {{\n",
            relationship.name
        ));
        push_call_ending(
            output,
            12,
            "let alias = open_exists",
            &[
                str_literal(&table_ref(target)),
                "depth".to_owned(),
                "query".to_owned(),
            ],
            "?;",
        );
        // A many-to-many correlates through its join table instead.
        let mut pairs = Vec::new();
        if join.is_none() {
            pairs.push((&relationship.target_column, &relationship.column));
        }
        // `filter_relationships` keeps a tenant target only when this table
        // has a tenant column too; both use the configured column name.
        pairs.extend(
            target
                .tenant()
                .zip(table.tenant())
                .map(|(theirs, ours)| (&theirs.sql_name, &ours.sql_name)),
        );
        for (theirs, ours) in pairs {
            push_call(
                output,
                12,
                "correlate",
                &[
                    "&alias".to_owned(),
                    quoted(theirs),
                    "outer".to_owned(),
                    quoted(ours),
                    "query".to_owned(),
                ],
            );
        }
        if let Some(live) = live_condition(target, None) {
            output.push_str(&format!(
                "            query.push({});\n",
                str_literal(&format!(" AND {live}"))
            ));
        }
        if let (Some(join), Some(through)) = (join, &relationship.through) {
            push_join_exists(output, table, join, relationship, through);
        }
        if filter_relationships(target, tables).is_empty() {
            output.push_str("            filter.push_conditions(query);\n");
        } else {
            output.push_str("            filter.push_conditions(&alias, depth + 1, query)?;\n");
        }
        output.push_str("            query.push(\")\");\n        }\n");
    }
    output.push_str("        Ok(())\n    }\n");
}

/// Inside a many-to-many's `EXISTS` over the related table (`alias`), a
/// nested ` AND EXISTS (SELECT 1 FROM <join> AS <alias>_j …)` requiring a
/// join row pairing the related row with the outer row, in the outer row's
/// tenant, and live (its unqualified soft-delete column resolves to the join
/// table, the innermost one).
fn push_join_exists(
    output: &mut String,
    table: &Table,
    join: &Table,
    relationship: &Relationship,
    through: &Through,
) {
    let quoted = |name: &str| str_literal(&ident(name));
    output.push_str("            let through = format!(\"{alias}_j\");\n");
    output.push_str(&format!(
        "            query.push({});\n",
        str_literal(&format!(
            " AND EXISTS (SELECT 1 FROM {} AS ",
            table_ref(join)
        ))
    ));
    output.push_str("            query.push(&through);\n");
    output.push_str("            query.push(\" WHERE TRUE\");\n");
    let mut pairs = vec![
        (
            &through.target_column,
            "&alias",
            &relationship.target_column,
        ),
        (&through.column, "outer", &relationship.column),
    ];
    pairs.extend(
        join.tenant()
            .zip(table.tenant())
            .map(|(theirs, ours)| (&theirs.sql_name, "outer", &ours.sql_name)),
    );
    for (theirs, outer, ours) in pairs {
        push_call(
            output,
            12,
            "correlate",
            &[
                "&through".to_owned(),
                quoted(theirs),
                outer.to_owned(),
                quoted(ours),
                "query".to_owned(),
            ],
        );
    }
    let close = match live_condition(join, None) {
        Some(live) => format!(" AND {live})"),
        None => ")".to_owned(),
    };
    output.push_str(&format!(
        "            query.push({});\n",
        str_literal(&close)
    ));
}
