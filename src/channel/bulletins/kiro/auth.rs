//! Kiro auth (TODO M7): two OAuth flows — social (Google/GitHub via
//! `prod.us-east-1.auth.desktop.kiro.dev`) or AWS IdC/OIDC client registration;
//! bearer token + `profile_arn`. Base `https://q.us-east-1.amazonaws.com`.
//! Requests/responses convert OpenAI Responses <-> Kiro Smithy event-stream (M2).
