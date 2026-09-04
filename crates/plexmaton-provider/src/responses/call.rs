//! Bounded assembly for one fragmented Responses function call.

use plexmaton_agent::ToolCall;
use plexmaton_core::ToolCallId;

use crate::codec::{DecodeError, DecodeLimits};

#[derive(Debug, Default)]
pub(super) struct CallAssembly {
    item_id: Option<String>,
    call_id: Option<String>,
    name: Option<String>,
    arguments: String,
    arguments_done: bool,
}

impl CallAssembly {
    pub(super) fn merge_identity(
        &mut self,
        item_id: Option<String>,
        call_id: Option<String>,
        name: Option<String>,
        index: usize,
        limits: DecodeLimits,
    ) -> Result<(), DecodeError> {
        merge_field(&mut self.item_id, item_id, index, "item_id", limits)?;
        merge_field(&mut self.call_id, call_id, index, "call_id", limits)?;
        merge_field(&mut self.name, name, index, "name", limits)
    }

    pub(super) fn merge_item_id(
        &mut self,
        item_id: Option<String>,
        index: usize,
        limits: DecodeLimits,
    ) -> Result<(), DecodeError> {
        merge_field(&mut self.item_id, item_id, index, "item_id", limits)
    }

    pub(super) fn seed_arguments(
        &mut self,
        arguments: Option<String>,
        index: usize,
        limits: DecodeLimits,
    ) -> Result<(), DecodeError> {
        if let Some(arguments) = arguments.filter(|arguments| !arguments.is_empty()) {
            self.append_arguments(&arguments, index, limits)?;
        }
        Ok(())
    }

    pub(super) fn append_delta(
        &mut self,
        delta: &str,
        index: usize,
        limits: DecodeLimits,
    ) -> Result<(), DecodeError> {
        if self.arguments_done {
            return Err(DecodeError::ConflictingToolFragment {
                index,
                field: "arguments_after_done",
            });
        }
        self.append_arguments(delta, index, limits)
    }

    pub(super) fn mark_arguments_done(
        &mut self,
        arguments: &str,
        index: usize,
        limits: DecodeLimits,
    ) -> Result<(), DecodeError> {
        if self.arguments_done {
            return Err(DecodeError::ConflictingToolFragment {
                index,
                field: "arguments_done",
            });
        }
        self.reconcile_arguments(arguments, index, limits)?;
        self.arguments_done = true;
        Ok(())
    }

    pub(super) fn complete_arguments(
        &mut self,
        arguments: &str,
        index: usize,
        limits: DecodeLimits,
    ) -> Result<(), DecodeError> {
        self.reconcile_arguments(arguments, index, limits)?;
        self.arguments_done = true;
        Ok(())
    }

    pub(super) fn finish(self, index: usize) -> Result<ToolCall, DecodeError> {
        let call_id = required(self.call_id, index, "call_id")?;
        let call_id = ToolCallId::new(call_id).map_err(|_| DecodeError::IncompleteToolCall {
            index,
            field: "call_id",
        })?;
        Ok(ToolCall {
            call_id,
            name: required(self.name, index, "name")?,
            arguments: self.arguments,
        })
    }

    fn reconcile_arguments(
        &mut self,
        arguments: &str,
        index: usize,
        limits: DecodeLimits,
    ) -> Result<(), DecodeError> {
        if self.arguments.is_empty() {
            self.append_arguments(arguments, index, limits)
        } else if self.arguments == arguments {
            Ok(())
        } else {
            Err(DecodeError::ConflictingToolFragment {
                index,
                field: "arguments",
            })
        }
    }

    fn append_arguments(
        &mut self,
        arguments: &str,
        index: usize,
        limits: DecodeLimits,
    ) -> Result<(), DecodeError> {
        let Some(next) = self.arguments.len().checked_add(arguments.len()) else {
            return Err(DecodeError::ToolArgumentsTooLarge {
                index,
                limit: limits.max_tool_argument_bytes,
            });
        };
        if next > limits.max_tool_argument_bytes {
            return Err(DecodeError::ToolArgumentsTooLarge {
                index,
                limit: limits.max_tool_argument_bytes,
            });
        }
        self.arguments.push_str(arguments);
        Ok(())
    }
}

fn merge_field(
    current: &mut Option<String>,
    incoming: Option<String>,
    index: usize,
    field: &'static str,
    limits: DecodeLimits,
) -> Result<(), DecodeError> {
    let Some(incoming) = incoming else {
        return Ok(());
    };
    if incoming.len() > limits.max_tool_identity_bytes {
        return Err(DecodeError::ToolIdentityTooLarge {
            index,
            field,
            limit: limits.max_tool_identity_bytes,
        });
    }
    match current {
        Some(value) if value != &incoming => {
            Err(DecodeError::ConflictingToolFragment { index, field })
        }
        Some(_) => Ok(()),
        None => {
            *current = Some(incoming);
            Ok(())
        }
    }
}

fn required(
    value: Option<String>,
    index: usize,
    field: &'static str,
) -> Result<String, DecodeError> {
    value
        .filter(|value| !value.trim().is_empty())
        .ok_or(DecodeError::IncompleteToolCall { index, field })
}
