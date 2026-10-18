//! Who a handler acts for and what its service method is scoped to: the
//! tenant the `Tenant` extractor hands over, and the caller the access
//! extractor checks (and passes on when rows are owned).

use super::feature::Feature;
use super::layout::Uses;
use crate::ApiAccess;
use crate::generator::param_type;
use crate::schema::Column;

/// A handler's scope: its extractor parameters, and the arguments it
/// passes before the request's own to the service method.
pub struct HandlerScope {
    pub params: Vec<String>,
    pub args: Vec<String>,
}

/// The scope of a handler taking `tenant` (when `Some`) and a caller with
/// `access`, whose id the service needs when `user`. A public handler has
/// no caller.
pub fn handler(
    feature: &Feature<'_>,
    uses: &mut Uses,
    tenant: Option<&Column>,
    access: ApiAccess,
    user: bool,
) -> HandlerScope {
    let mut params = Vec::new();
    let mut args = Vec::new();
    // The caller's extractor answers `401` (or `403`) before the handler
    // runs, and before the tenant's: a caller without credentials is a
    // `401`, not a missing tenant.
    match (access, user) {
        (ApiAccess::Public, _) => {}
        (ApiAccess::Admin, true) => {
            uses.add("crate::shared::auth::AdminUser");
            params.push("admin: AdminUser".to_owned());
            args.push("admin.0".to_owned());
        }
        (ApiAccess::Admin, false) => {
            uses.add("crate::shared::auth::AdminUser");
            params.push("_: AdminUser".to_owned());
        }
        (_, true) => {
            uses.add("crate::shared::auth::AuthUser");
            params.push("user: AuthUser".to_owned());
            args.push("user".to_owned());
        }
        (_, false) => {
            uses.add("crate::shared::auth::AuthUser");
            params.push("_: AuthUser".to_owned());
        }
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
