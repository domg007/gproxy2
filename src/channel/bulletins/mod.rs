//! Channel adapters ("bulletins") — one folder per channel; each manages its
//! own auth in `auth.rs`. API-key channels share [`common`]; OAuth/envelope
//! channels are stubs until their auth/transform infra lands (M7 / M2).

pub mod common;

#[cfg(test)]
mod tests;

// API-key channels (functional)
pub mod aistudio;
pub mod claude_api;
pub mod custom;
pub mod deepseek;
pub mod groq;
pub mod nvidia;
pub mod openai;
pub mod openrouter;
pub mod vercel;
pub mod vertexexpress;

// OAuth / envelope channels (stubs — prepare errors until M7/M2)
pub mod antigravity;
pub mod claudecode;
pub mod codex;
pub mod copilot_cli;
pub mod geminicli;
pub mod kiro;
pub mod vertex;
