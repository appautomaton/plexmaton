//! Projects the runtime's waiting input into the workspace.
//!
//! The composition root is the only crate that knows both vocabularies, so the mapping lives here
//! rather than widening either side's dependencies.

use plexmaton_runtime::{LiveRuntime, QueuedBoundary};
use plexmaton_tui::Workspace;

/// Replaces the workspace's projection with what the runtime is still holding.
///
/// Called once per loop iteration rather than on a submission: input also leaves these queues when
/// a boundary claims it, and no submission happens then.
pub(super) fn sync(runtime: &LiveRuntime, workspace: &mut Workspace) {
    workspace.set_queued_input(
        runtime
            .queued_input()
            .map(|queued| plexmaton_tui::QueuedInput {
                text: queued.text.to_owned(),
                boundary: match queued.boundary {
                    QueuedBoundary::Step => plexmaton_tui::QueuedBoundary::Step,
                    QueuedBoundary::Turn => plexmaton_tui::QueuedBoundary::Turn,
                    QueuedBoundary::Admission => plexmaton_tui::QueuedBoundary::Admission,
                },
            })
            .collect(),
    );
}
