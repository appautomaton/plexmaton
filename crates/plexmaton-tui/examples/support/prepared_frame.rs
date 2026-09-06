//! Offline review fixture at the public preparation seam; never part of interactive rendering.

pub fn draw(
    workspace: &mut plexmaton_tui::Workspace,
    terminal: &mut ratatui::Terminal<ratatui::backend::TestBackend>,
) -> Result<(), Box<dyn std::error::Error>> {
    for _ in 0..1024 {
        workspace.draw(terminal)?;
        if let Some(work) = workspace.take_preparation() {
            match plexmaton_tui::preparation::prepare_batch(&work.requests) {
                Ok(prepared) => {
                    if !workspace.complete_preparation(work.token, prepared) {
                        return Err("review preparation was rejected".into());
                    }
                }
                Err(plexmaton_tui::preparation::BatchRefusal::Capacity) => workspace
                    .fail_preparation(work.token, plexmaton_tui::preparation::Refusal::Capacity),
            }
        } else if !workspace.needs_draw() {
            return Ok(());
        }
    }
    Err("review preparation did not settle".into())
}
