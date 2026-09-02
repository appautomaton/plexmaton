//! Responses API request and stream grammar.

mod call;
mod request;
mod stream;
mod text;
mod wire;

pub(crate) use request::encode;
pub(crate) use stream::ResponsesDecoder;
