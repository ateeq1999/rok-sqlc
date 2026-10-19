//! What a feature serves: its [`Endpoint`]s, and which of its write bodies
//! a pre-check reads ([`Bodies`]).

use crate::ApiEndpoint;

/// One endpoint of a feature, in the order the files list them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Endpoint {
    /// `GET /`: a page of rows.
    List,
    /// `POST /search`: a page of rows matching per-field filters.
    Search,
    /// `POST /`: create a row.
    Create,
    /// `GET /{key}`: one row.
    GetOne,
    /// `PATCH /{key}`: update a row.
    Update,
    /// `DELETE /{key}`: delete a row (soft-delete it, on a soft-delete
    /// table, which also takes `?permanent=true`).
    Remove,
    /// `POST /{key}/restore`: undo a soft delete.
    Restore,
}

impl Endpoint {
    /// Every endpoint, in order.
    pub const ALL: [Endpoint; 7] = [
        Endpoint::List,
        Endpoint::Search,
        Endpoint::Create,
        Endpoint::GetOne,
        Endpoint::Update,
        Endpoint::Remove,
        Endpoint::Restore,
    ];

    /// The handler's name, which is also the service method's.
    pub fn name(self) -> &'static str {
        match self {
            Endpoint::List => "list",
            Endpoint::Search => "search",
            Endpoint::Create => "create",
            Endpoint::GetOne => "get_one",
            Endpoint::Update => "update",
            Endpoint::Remove => "remove",
            Endpoint::Restore => "restore",
        }
    }

    /// The `[api.skip]` name of the endpoint.
    pub fn config(self) -> ApiEndpoint {
        match self {
            Endpoint::List => ApiEndpoint::List,
            Endpoint::Search => ApiEndpoint::Search,
            Endpoint::Create => ApiEndpoint::Create,
            Endpoint::GetOne => ApiEndpoint::GetOne,
            Endpoint::Update => ApiEndpoint::Update,
            Endpoint::Remove => ApiEndpoint::Remove,
            Endpoint::Restore => ApiEndpoint::Restore,
        }
    }

    /// The actix-web method guard, such as `get`.
    pub fn method(self) -> &'static str {
        match self {
            Endpoint::List | Endpoint::GetOne => "get",
            Endpoint::Create | Endpoint::Search | Endpoint::Restore => "post",
            Endpoint::Update => "patch",
            Endpoint::Remove => "delete",
        }
    }

    /// The HTTP method, such as `GET`.
    pub fn http_method(self) -> String {
        self.method().to_uppercase()
    }

    /// Whether the endpoint addresses one row by its key.
    pub fn takes_key(self) -> bool {
        matches!(
            self,
            Endpoint::GetOne | Endpoint::Update | Endpoint::Remove | Endpoint::Restore
        )
    }

    /// The path under the feature's URL, given the key's path.
    pub fn path(self, key_path: &str) -> String {
        match self {
            Endpoint::List | Endpoint::Create => String::new(),
            Endpoint::Search => "/search".to_owned(),
            Endpoint::GetOne | Endpoint::Update | Endpoint::Remove => key_path.to_owned(),
            Endpoint::Restore => format!("{key_path}/restore"),
        }
    }
}

/// Which of a feature's write bodies hold something a pre-check reads:
/// the create body, the update body, or both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bodies {
    /// Whether the create body holds it.
    pub create: bool,
    /// Whether the update body holds it.
    pub update: bool,
}

impl Bodies {
    /// Whether `endpoint`'s body holds it: never any but a create's or an
    /// update's.
    pub fn holds(self, endpoint: Endpoint) -> bool {
        match endpoint {
            Endpoint::Create => self.create,
            Endpoint::Update => self.update,
            _ => false,
        }
    }

    /// Whether either body holds it.
    pub fn any(self) -> bool {
        self.create || self.update
    }
}
