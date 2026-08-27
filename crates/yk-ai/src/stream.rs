//! What arrives while a model is answering.
//!
//! Providers stream in different formats and at different granularities; the
//! only thing a reader cares about is which of three things arrived. So the
//! wire differences stop here, and everything above sees these.

/// A fragment of an answer.
#[derive(Debug, Clone, PartialEq)]
pub enum Delta<'a> {
    /// Part of the answer itself.
    Text(&'a str),
    /// Part of the model's working. Separate because it is not the answer, and
    /// a reader shown a draft as a conclusion has been misled.
    Reasoning(&'a str),
}

impl Delta<'_> {
    /// The name this fragment travels under, for callers that carry a string.
    pub fn kind(&self) -> &'static str {
        match self {
            Delta::Text(_) => "content",
            Delta::Reasoning(_) => "reasoning",
        }
    }

    pub fn text(&self) -> &str {
        match self {
            Delta::Text(t) | Delta::Reasoning(t) => t,
        }
    }
}

/// Where fragments go as they arrive.
///
/// A plain function rather than a channel or a stream: the caller is nearly
/// always appending to something it already owns, and a channel would add a
/// task, a buffer and an ordering question to a problem that has none.
pub type Sink<'a> = &'a (dyn Fn(Delta<'_>) + Send + Sync);

/// The sink for callers that only want the finished message.
pub fn ignore(_: Delta<'_>) {}
