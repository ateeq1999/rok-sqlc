//! A feature's relationship routes (ADR 0016), one [`Relation`] per
//! relationship of its table whose related table has a feature of its own:
//! a to-one relationship is embedded by `GET /{key}?include=<name>`, a
//! has-many one is read by `GET /{key}/<name>`, and a many-to-many one is
//! also synced, attached and detached. Related rows answer through their
//! own feature's response, and that feature's access and owner apply.

use heck::{ToKebabCase, ToUpperCamelCase};

use super::feature::{Endpoint, Feature};
use super::scope::Caller;
use crate::generator::joins::joins;
use crate::generator::relationships::{join_pair, resolve};
use crate::schema::{Column, Table};
use crate::{ApiAccess, ApiConfig};

/// One relationship route set of a feature.
#[derive(Debug, Clone)]
pub struct Relation<'a> {
    /// The relationship's name (`[relationships]` renames applied).
    pub name: String,
    pub kind: Kind<'a>,
    /// The related table.
    pub target: &'a Table,
    /// Who may read the related table: the route needs the stricter of it
    /// and the feature's own access.
    pub target_access: ApiAccess,
    /// The related table's owner column: only the caller's related rows
    /// are answered, attached or detached.
    pub target_owner: Option<&'a Column>,
    /// The tenant column whose type the route's `Tenant` holds, when any
    /// data layer call it makes is tenant-scoped.
    pub tenant: Option<&'a Column>,
}

/// What a relation's routes read and write.
#[derive(Debug, Clone)]
pub enum Kind<'a> {
    /// A to-one relationship, embedded by `?include=` through the data
    /// layer's joined read.
    ToOne {
        /// Whether the related row may be absent.
        optional: bool,
        /// The joined read's field holding the row read.
        row_field: String,
        /// Whether the joined read takes the tenant.
        tenant: bool,
    },
    /// A to-many relationship, read through the data layer's loader.
    ToMany {
        /// The column of the feature's rows the loader keys its map by.
        key: &'a Column,
        /// Whether the loader takes the tenant.
        tenant: bool,
        /// A many-to-many relationship's writes.
        writes: Option<Writes<'a>>,
    },
}

/// The writes of a many-to-many relationship, through its join table.
#[derive(Debug, Clone)]
pub struct Writes<'a> {
    /// The join table.
    pub join: &'a Table,
    /// The join table's column holding the feature's key, and the one
    /// holding the related row's.
    pub own: &'a Column,
    pub other: &'a Column,
    /// Whether the join table's `attach` and `sync` take the feature's key
    /// first (its pair's `near` column).
    pub near: bool,
    /// Whether the join table has `attach` and `sync`: they insert only the
    /// pair.
    pub inserts: bool,
}

impl<'a> Relation<'a> {
    /// Who the routes act for: the stricter of the feature's and the related
    /// table's access, and the caller's id when either table is owned.
    pub fn caller(&self, feature: &Feature<'_>) -> Caller {
        fn rank(access: ApiAccess) -> u8 {
            match access {
                ApiAccess::Public => 0,
                ApiAccess::User => 1,
                ApiAccess::Admin => 2,
            }
        }
        let access = if rank(self.target_access) > rank(feature.access) {
            self.target_access
        } else {
            feature.access
        };
        Caller {
            access,
            user: feature.owner.is_some() || self.target_owner.is_some(),
        }
    }

    /// The related feature's response, such as `AccountResponse`.
    pub fn response(&self) -> String {
        format!("{}Response", self.target.struct_name)
    }

    /// The related table's row, such as `Account`.
    pub fn row(&self) -> &str {
        &self.target.struct_name
    }

    /// The related table's key type, such as `LabelId`.
    pub fn target_key(&self) -> String {
        crate::generator::keys::key_type(self.target)
    }

    /// The route segment, such as `labels`.
    pub fn segment(&self) -> String {
        self.name.to_kebab_case()
    }

    /// The to-one embedding's response, such as `TaskWithOwnerResponse`.
    pub fn with_response(&self, feature: &Feature<'_>) -> String {
        format!(
            "{}With{}Response",
            feature.row(),
            self.name.to_upper_camel_case()
        )
    }

    /// The `?include=` variant, such as `Owner`.
    pub fn variant(&self) -> String {
        self.name.to_upper_camel_case()
    }

    /// Whether the relation is many-to-many with writes.
    pub fn writes(&self) -> Option<&Writes<'a>> {
        match &self.kind {
            Kind::ToMany { writes, .. } => writes.as_ref(),
            Kind::ToOne { .. } => None,
        }
    }

    /// Whether `PUT /{key}/<name>` replaces the pairs: only from the join
    /// table's `near` side, where the data layer has `sync`, and only when
    /// the related rows aren't owned (a sync would drop other owners'
    /// pairs).
    pub fn syncs(&self) -> bool {
        self.writes()
            .is_some_and(|writes| writes.near && writes.inserts && self.target_owner.is_none())
    }

    /// Whether `PUT /{key}/<name>/{other}` attaches.
    pub fn attaches(&self) -> bool {
        self.writes().is_some_and(|writes| writes.inserts)
    }

    /// The handlers of a to-many relation, in the order `handlers.rs` holds
    /// them: its list, then a many-to-many's sync, attach and detach where
    /// served. None for a to-one relation, which `GET /{key}` embeds.
    pub fn actions(&self) -> Vec<Action> {
        if !matches!(self.kind, Kind::ToMany { .. }) {
            return Vec::new();
        }
        let mut actions = vec![Action::List];
        if self.writes().is_some() {
            if self.syncs() {
                actions.push(Action::Sync);
            }
            if self.attaches() {
                actions.push(Action::Attach);
            }
            actions.push(Action::Detach);
        }
        actions
    }

    /// The name of `action`'s handler, which is also its operation id, such
    /// as `list_labels`.
    pub fn handler(&self, action: Action) -> String {
        format!("{}_{}", action.name(), self.name)
    }

    /// `action`'s route under the feature's route base, such as
    /// `/{id}/labels/{other}`.
    pub fn path(&self, feature: &Feature<'_>, action: Action) -> String {
        let path = format!("{}/{}", feature.key_path(), self.segment());
        if action.names_other() {
            format!("{path}/{{other}}")
        } else {
            path
        }
    }
}

/// A handler of a to-many relation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// `GET /{key}/<name>`: the related rows.
    List,
    /// `PUT /{key}/<name>`: pairs the row with exactly the rows named.
    Sync,
    /// `PUT /{key}/<name>/{other}`: pairs the row with another.
    Attach,
    /// `DELETE /{key}/<name>/{other}`: unpairs them.
    Detach,
}

impl Action {
    /// The handler's name before the relation's: `list`, `sync`, `attach`
    /// or `detach`.
    pub fn name(self) -> &'static str {
        match self {
            Action::List => "list",
            Action::Sync => "sync",
            Action::Attach => "attach",
            Action::Detach => "detach",
        }
    }

    /// The HTTP method, lowercase.
    pub fn method(self) -> &'static str {
        match self {
            Action::List => "get",
            Action::Sync | Action::Attach => "put",
            Action::Detach => "delete",
        }
    }

    /// Whether the route names the related row too, as `{other}`.
    pub fn names_other(self) -> bool {
        matches!(self, Action::Attach | Action::Detach)
    }
}

impl<'a> Feature<'a> {
    /// The to-one relations, which `GET /{key}?include=` embeds.
    pub fn includes(&self) -> Vec<&Relation<'a>> {
        self.relations
            .iter()
            .filter(|relation| matches!(relation.kind, Kind::ToOne { .. }))
            .collect()
    }
}

/// The relations of `feature`, among `features` (each with its table in
/// `tables`): none when `[api.skip]` skips them.
pub fn relations<'a>(
    feature: &Feature<'a>,
    features: &[Feature<'a>],
    tables: &'a [Table],
    api: &ApiConfig,
) -> Vec<Relation<'a>> {
    let table = feature.table;
    if Feature::skips_relationships(api, table) || feature.key.is_none() {
        return Vec::new();
    }
    let joined = joins(table, tables);
    table
        .relationships
        .iter()
        .filter_map(|relationship| {
            let resolved = resolve(table, relationship, tables);
            let target = features
                .iter()
                .find(|other| other.table.module_name == resolved.target.module_name)?;
            let target_tenant = target.table.tenant();
            let (kind, tenants) = if relationship.kind.is_to_many() {
                let writes = resolved.join.as_ref().and_then(|join| {
                    let pair = join_pair(join.table)?;
                    let single = |table: &Table| matches!(table.key_columns().as_slice(), [key] if key.is_copy());
                    if !single(table) || !single(target.table) {
                        return None;
                    }
                    Some(Writes {
                        join: join.table,
                        own: join.column,
                        other: join.target_column,
                        near: pair.near.sql_name == join.column.sql_name,
                        inserts: join_inserts(join.table, pair.near, pair.far),
                    })
                });
                let join_tenant = resolved.join.as_ref().and_then(|join| join.table.tenant());
                let loader_tenant = target_tenant.or(join_tenant);
                let mut tenants = vec![loader_tenant, target_tenant];
                if let Some(writes) = &writes {
                    tenants.push(writes.join.tenant());
                }
                let kind = Kind::ToMany {
                    key: resolved.column,
                    tenant: loader_tenant.is_some(),
                    writes,
                };
                (kind, tenants)
            } else {
                if !feature.serves(Endpoint::GetOne) {
                    return None;
                }
                let join = joined.iter().find(|join| join.name() == relationship.name)?;
                let kind = Kind::ToOne {
                    optional: join.optional(),
                    row_field: join.row_field(),
                    tenant: join.tenant().is_some(),
                };
                (kind, vec![join.tenant(), target_tenant])
            };
            let tenant = one_tenant(feature.tenant, &tenants)?;
            Some(Relation {
                name: relationship.name.clone(),
                kind,
                target: target.table,
                target_access: target.access,
                target_owner: target.owner,
                tenant,
            })
        })
        .collect()
}

/// The one tenant column of a route's calls: the feature's, or the first
/// of `others`. `None` (no route) when two of them hold different types,
/// or a type a `Tenant` can't hand to several calls by value.
#[allow(
    clippy::option_option,
    reason = "`Some(None)`: a route without a tenant"
)]
fn one_tenant<'a>(
    own: Option<&'a Column>,
    others: &[Option<&'a Column>],
) -> Option<Option<&'a Column>> {
    let mut all = own.into_iter().chain(others.iter().flatten().copied());
    let Some(first) = all.next() else {
        return Some(None);
    };
    let same = all.all(|other| other.value_type() == first.value_type());
    (same && first.is_copy()).then_some(Some(first))
}

/// Whether a join table's `attach` and `sync` exist: every insertable
/// column but the pair can be left out.
fn join_inserts(join: &Table, near: &Column, far: &Column) -> bool {
    join.insert_columns().iter().all(|column| {
        column.sql_name == near.sql_name
            || column.sql_name == far.sql_name
            || column.has_default
            || column.nullable
    })
}
