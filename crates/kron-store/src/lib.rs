#![allow(clippy::missing_errors_doc)]

mod error;
mod models;
mod store;

pub use error::StoreError;
pub use models::{JobRecord, RunRecord, RunStatus, RunSummary};
pub use store::Store;
