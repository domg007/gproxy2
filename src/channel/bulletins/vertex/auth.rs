//! Vertex auth (TODO M7): sign a service-account JWT (`client_email` +
//! `private_key` from `secret_json`) and exchange it at
//! `https://oauth2.googleapis.com/token` for a short-lived bearer; cache the
//! access token by `client_email`. Wired once the OAuth refresh infra lands.
