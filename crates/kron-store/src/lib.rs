#![allow(clippy::missing_errors_doc)]

mod error;
mod models;
mod store;

pub use error::StoreError;
pub use models::{JobRecord, RunRecord, RunStatus};
pub use store::Store;
