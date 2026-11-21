//! Machine translation adapters (PD-2.6).
//!
//! * [`OpenAiChatMt`]: the streaming translator for OpenAI-compatible chat
//!   endpoints, `llama-server` first (`openai_chat`).
//! * `apple`: system translation, the translation port over Apple's on-device Translation framework.
//! * `config`, `prompt`: its configuration, request body and prompt files.
//! * `sse`: the incremental parser of the response stream (in `chat_http`, shared with the dialogue backend).
//! * `output`, `assess`: the structural clean-up of the model's output and the
//!   cheap checks that detect output that is not a translation.
//! * `repetition`: the incremental detector of a model that loops; it aborts the request early
//!   and the stream reports why through `MtStream::suspect`.
//! * `clause_split`: the splitter from translated token stream to clauses (lives in `prxvc-media`, re-exported here).
//! * `report`: typed failures, quality flags and the observer that receives them.

pub mod apple;
pub mod assess;
pub mod config;
pub mod openai_chat;
pub mod output;
pub mod prompt;
pub mod repetition;
pub mod report;

/// The incremental SSE parser lives with the shared chat transport.
pub use crate::chat_http::sse;

pub use assess::{AssessInput, assess, estimate_tokens, max_output_tokens};
pub use config::{ApiKey, ConfigError, OpenAiChatConfig, Timeouts};
pub use openai_chat::OpenAiChatMt;
pub use output::OutputFilter;
pub use prompt::{PromptLimits, PromptTemplate, UnknownTemplate};
pub use prxvc_media::clause_split::{self, ClauseSplitter, SplitParams};
pub use repetition::{Repetition, RepetitionGuard};
pub use report::{MtFailure, MtObserver, MtReport, Stripped, Suspect};
pub use sse::{SseEvent, SseParser};
