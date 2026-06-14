//! §F7a portal API (`/user/*`) — native-only, behind `require_session`.
//! Every read/write scopes to the session user's id; request-supplied ids are ignored.

pub mod audit;
pub mod keys;
pub mod me;
