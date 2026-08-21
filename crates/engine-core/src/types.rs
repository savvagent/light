//! Plain data passed across the trait seams. No behavior.

use std::path::PathBuf;

/// A request to an LLM provider. Prompt-and-parse: the engine renders the whole prompt,
/// including any history, and parses structure out of the completion text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompleteRequest {
    pub prompt: String,
}

/// Token usage reported by a provider. `None` for providers that do not report it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

/// A provider's completion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompleteResponse {
    pub text: String,
    pub usage: Option<Usage>,
}

/// A single full-file edit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    pub path: PathBuf,
    pub new_contents: String,
}
