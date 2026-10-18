//! Who a handler acts for and what its service method is scoped to: the
//! tenant the `Tenant` extractor hands over, and the caller the access
//! extractor checks (and passes on when rows are owned).

use super::feature::Feature;
use super::layout::Uses;
use crate::ApiAccess;
use crate::generator::param_type;
use crate::schema::Column;

/// Who a handler acts for: the access its routes need, and whether the
/// service needs the caller (`user`, when rows are owned).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caller {
    pub access: ApiAccess,
    pub user: bool,
}

/// How a handler gets its caller: the extractor type, the name it binds
/// (`_` when the service doesn't need the caller), and the argument the
/// service gets (`None` then).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Extraction {
    pub extractor: &'static str,
    pub binding: &'static str,
    pub arg: Option<&'static str>,
}

impl Caller {
    /// How a handler gets this caller; `None` for a public route, which has
    /// no caller.
    pub fn extraction(self) -> Option<Extraction> {
        let (extractor, binding, arg) = match (self.access, self.user) {
            (ApiAccess::Public, _) => return None,
            (ApiAccess::Admin, true) => ("AdminUser", "admin", Some("admin.0")),
            (ApiAccess::Admin, false) => ("AdminUser", "_", None),
            (_, true) => ("AuthUser", "user", Some("user")),
            (_, false) => ("AuthUser", "_", None),
        };
        Some(Extraction {
            extractor,
            binding,
            arg,
        })
    }
}

/// A handler's scope: its extractor parameters, and the arguments it
/// passes before the request's own to the service method.
pub struct HandlerScope {
    pub params: Vec<String>,
    pub args: Vec<String>,
}

/// The scope of a handler taking `tenant` (when `Some`) and `caller`.
pub fn handler(
    feature: &Feature<'_>,
    uses: &mut Uses,
    tenant: Option<&Column>,
    caller: Caller,
) -> HandlerScope {
    let mut params = Vec::new();
    let mut args = Vec::new();
    // The caller's extractor answers `401` (or `403`) before the handler
    // runs, and before the tenant's: a caller without credentials is a
    // `401`, not a missing tenant.
    if let Some(Extraction {
        extractor,
        binding,
        arg,
    }) = caller.extraction()
    {
        uses.add(&format!("crate::shared::auth::{extractor}"));
        params.push(format!("{binding}: {extractor}"));
        args.extend(arg.map(str::to_owned));
    }
    if let Some(tenant) = tenant {
        uses.add("crate::shared::tenant::Tenant");
        import_type(feature, uses, tenant);
        params.push(format!("tenant: Tenant<{}>", tenant.value_type()));
        // The service takes the tenant first.
        args.insert(0, "tenant.into_inner()".to_owned());
    }
    HandlerScope { params, args }
}

/// Imports the type of `column` (a tenant) when it is a typed id.
pub fn import_type(feature: &Feature<'_>, uses: &mut Uses, column: &Column) {
    if let Some(id) = &column.typed_id {
        if id.module == feature.module() {
            uses.add(&format!("super::model::{}", id.name));
        } else {
            uses.add(&format!(
                "{}::{}::models::{}",
                feature.db_path, id.module, id.name
            ));
        }
    }
}

/// The tenant argument of a data layer call on `table`'s rows, with its
/// trailing `, `: the service's `tenant`, borrowed when the call borrows
/// it; nothing for a table without a tenant column.
pub fn tenant_arg(table: &crate::schema::Table) -> String {
    match table.tenant() {
        Some(tenant) if param_type(tenant).starts_with('&') => "&tenant, ".to_owned(),
        Some(_) => "tenant, ".to_owned(),
        None => String::new(),
    }
}
