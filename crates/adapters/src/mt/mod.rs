//! Machine translation adapters (PD-2.6).
//!
//! * [`OpenAiChatMt`]: the streaming translator for OpenAI-compatible chat
//!   endpoints, `llama-server` first (`openai_chat`).
//! * `config`, `prompt`: its configuration, request body and prompt files.
//! * `sse`: the incremental parser of the response stream.
//! * `output`, `assess`: the structural clean-up of the model's output and the
//!   cheap checks that detect output that is not a translation.
//! * `clause_split`: the splitter from translated token stream to clauses.
//! * `report`: typed failures, quality flags and the observer that receives them.

pub mod assess;
pub mod clause_split;
pub mod config;
pub mod openai_chat;
pub mod output;
pub mod prompt;
pub mod report;
pub mod sse;

pub use assess::{AssessInput, assess, estimate_tokens, max_output_tokens};
pub use clause_split::{ClauseSplitter, SplitParams};
pub use config::{ApiKey, ConfigError, OpenAiChatConfig, Timeouts};
pub use openai_chat::OpenAiChatMt;
pub use output::OutputFilter;
pub use prompt::{PromptLimits, PromptTemplate, UnknownTemplate};
pub use report::{MtFailure, MtObserver, MtReport, Stripped, Suspect};
pub use sse::{SseEvent, SseParser};
