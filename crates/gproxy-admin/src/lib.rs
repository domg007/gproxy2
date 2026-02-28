//! Admin-facing domain methods (admin + user operations) for gproxy.

pub mod admin;
pub mod error;
pub mod memory;
pub mod user;

pub use admin::*;
pub use error::AdminApiError;
pub use memory::{MemoryUser, MemoryUserKey, normalize_user_api_key};
pub use user::*;
