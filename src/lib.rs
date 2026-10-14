//! Core library: catalog, filter, SQL connection and functions.

pub mod catalog;
pub mod connection;
pub mod filter;
pub mod functions;
pub mod plan;

pub use catalog::{Catalog, CatalogObject, ObjectKind};
pub use connection::{connect, ping, ConnectArgs, SqlClient};
pub use filter::{FilterItem, FilterOperator, SearchConditions};
pub use plan::PlanOperator;
