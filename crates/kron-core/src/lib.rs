//! Core logic for kron: config, scheduling, job execution, and notifications.

#![allow(clippy::missing_errors_doc)]

pub mod config;
pub mod crontab;
pub mod error;
pub mod notify;
pub mod runner;
pub mod scheduler;
pub mod systemd;

pub use error::CoreError;
