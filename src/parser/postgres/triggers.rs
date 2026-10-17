//! Row-level `BEFORE INSERT` triggers, which supply the values inserts leave
//! out.

use std::collections::HashMap;

use sqlparser::ast::{CreateTrigger, DropTrigger, TriggerEvent, TriggerObject, TriggerPeriod};

use super::StatementLocation;
use super::names::{find_table, object_leaf_name};
use crate::schema::Table;

/// Whether each of `tables` has a `BEFORE INSERT` trigger, in table order.
pub(super) fn has_before_insert_trigger(tables: &[Table]) -> Vec<bool> {
    tables
        .iter()
        .map(|table| !table.before_insert_triggers.is_empty())
        .collect()
}

/// Records `here` as where each table that had a `BEFORE INSERT` trigger
/// (`triggered`, by [`has_before_insert_trigger`] before the statement) and
/// now has none lost that source of values.
pub(super) fn record_lost_triggers(
    tables: &[Table],
    triggered: &[bool],
    source_removed: &mut HashMap<(String, Option<String>), StatementLocation>,
    here: &StatementLocation,
) {
    for (table, had) in tables.iter().zip(triggered) {
        if *had && table.before_insert_triggers.is_empty() {
            source_removed.insert((table.module_name.clone(), None), here.clone());
        }
    }
}

/// Records a `CREATE TRIGGER` on a known table: a row-level `BEFORE` trigger
/// that fires on `INSERT` (alone or with other events) can supply the values
/// of the columns inserts leave out. Its function is not inspected. A
/// `CREATE OR REPLACE` of a trigger with the same name replaces it.
pub(super) fn record_trigger(tables: &mut [Table], trigger: &CreateTrigger) {
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
pub(super) fn forget_trigger(tables: &mut [Table], trigger: &DropTrigger) {
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
