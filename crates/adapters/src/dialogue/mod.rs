//! Dialogue backends of the assistant (work package CT-20).
//!
//! * [`OpenAiChatDialogue`]: a conversational backend over an OpenAI-compatible
//!   `/v1/chat/completions` endpoint, `llama-server` first (`openai_chat`). It reuses the HTTP and
//!   SSE client of the translator (`chat_http`).
//! * `config`: its configuration (endpoint, model, credential, extra body fields, timeouts and the
//!   history bounds).
//! * `history`: the per-conversation history, bounded by turns and by tokens.
//! * `prompt`: the system prompt of a spoken assistant (a data file under
//!   `crates/adapters/prompts/`, embedded at build time) and the request body.

pub mod config;
pub mod history;
pub mod openai_chat;
pub mod prompt;

pub use config::{DialogueConfig, HistoryLimits};
pub use history::History;
pub use openai_chat::OpenAiChatDialogue;
