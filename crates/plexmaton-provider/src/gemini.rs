//! Native Gemini GenerateContent text and function-call protocol.

mod request;
mod stream;
mod usage;
mod wire;

pub(crate) use request::{encode, encode_atom};
pub(crate) use stream::GeminiDecoder;
