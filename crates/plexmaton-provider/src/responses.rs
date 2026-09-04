//! Responses API request and stream grammar.

mod call;
mod request;
mod stream;
mod text;
mod usage;
mod wire;

pub(crate) use request::{encode, encode_atom};
pub(crate) use stream::ResponsesDecoder;
