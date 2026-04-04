pub mod admin;
pub mod auth;
pub mod cors;
pub mod error;
pub mod login;
pub mod provider;
pub mod router;
pub mod user;

pub use cors::CorsLayer;
pub use router::api_router;
