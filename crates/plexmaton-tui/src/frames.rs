//! The composition, frozen: what the canonical scenario looks like at each width, and which words
//! the screen is allowed to say.
//!
//! Every other render test proves one mechanism. These families prove the whole frame, because a
//! layout can satisfy every mechanism and still not be the contract's composition. The fixtures
//! under `frames/` are text, so a reviewer reads the diff; colour is proven by the role tests.
//! Refresh them with `PLEXMATON_WRITE_FRAMES=1 cargo test -p plexmaton-tui frames`, and review the
//! diff as the behaviour change it is.

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use plexmaton_core::{
        AgentId, ApprovalId, AttentionId, AttentionRequest, SessionEvent, ToolCallId,
        ToolCallStatus, ToolCapability,
    };
    use ratatui::{buffer::Buffer, layout::Rect};

    use crate::{
        ViewState,
        intent::{AttentionIntent, Direction, InspectorIntent},
        surface::{SurfaceId, SurfaceTree},
        test_support::{Conversation, canonical_state, draw, draw_frame, region_text},
        theme::Palette,
    };

    /// One frame per width class, at a height that shows the whole composition.
    const FRAMES: [(&str, u16, u16); 3] = [
        ("canonical-wide", 120, 40),
        ("canonical-medium", 95, 40),
        ("canonical-narrow", 60, 40),
    ];

    /// The first blocking surface, at the same three product widths.
    const APPROVAL_FRAMES: [(&str, u16, u16); 3] = [
        ("approval-wide", 120, 40),
        ("approval-medium", 95, 40),
        ("approval-narrow", 60, 40),
    ];

    /// Slice 10's live primary-agent command approval, across the same product widths.
    const NATIVE_APPROVAL_FRAMES: [(&str, u16, u16); 3] = [
        ("native-approval-wide", 124, 40),
        ("native-approval-medium", 95, 40),
        ("native-approval-narrow", 60, 40),
    ];

    fn fixture_path(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("frames")
            .join(format!("{name}.txt"))
    }

    /// The first line that differs, numbered, so the failure says where to look.
    fn first_difference(expected: &str, actual: &str) -> String {
        for (index, (want, got)) in expected.lines().zip(actual.lines()).enumerate() {
            if want != got {
                return format!(
                    "line {}:\n  fixture: {want:?}\n  drawn:   {got:?}",
                    index + 1
                );
            }
        }
        format!(
            "line count: fixture {} lines, drawn {} lines",
            expected.lines().count(),
            actual.lines().count()
        )
    }

    /// Phase 01 §scope 1: the composition at wide, medium and narrow, checked in.
    #[test]
    fn the_canonical_frames_match_their_fixtures() {
        let write = std::env::var_os("PLEXMATON_WRITE_FRAMES").is_some();
        for (name, width, height) in FRAMES {
            let drawn = draw(&canonical_state(), width, height);
            // Structural first, so an empty or truncated fixture cannot pass by matching nothing.
            for signature in [
                "Agents",
                "Agent A · primary",
                "Message Agent A",
                "~/plexmaton",
            ] {
                assert!(
                    drawn.contains(signature),
                    "{name}: {signature:?} is not on screen"
                );
            }
            assert_eq!(
                drawn.lines().count(),
                usize::from(height),
                "{name}: every row painted"
            );

            let path = fixture_path(name);
            if write {
                std::fs::write(&path, &drawn)
                    .unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
                continue;
            }
            let fixture = std::fs::read_to_string(&path).unwrap_or_else(|error| {
                panic!(
                    "read {}: {error}\nwrite the fixtures with PLEXMATON_WRITE_FRAMES=1 and review them",
                    path.display()
                )
            });
            assert!(
                fixture == drawn,
                "{name} drifted from its fixture at {}\nif the change is intended, refresh with \
                 PLEXMATON_WRITE_FRAMES=1 and review the diff",
                first_difference(&fixture, &drawn)
            );
        }
    }

    fn approval_state(width: u16, height: u16) -> ViewState {
        let mut conversation = Conversation::canonical();
        conversation.emit(SessionEvent::AttentionRequested {
            agent_id: AgentId::new("agent-b").unwrap_or_else(|error| panic!("fixture: {error}")),
            attention_id: AttentionId::new("attention-b-approval")
                .unwrap_or_else(|error| panic!("fixture: {error}")),
            request: AttentionRequest::Approval {
                approval_id: ApprovalId::new("approval-b-1")
                    .unwrap_or_else(|error| panic!("fixture: {error}")),
                call_id: ToolCallId::new("tool-b-write")
                    .unwrap_or_else(|error| panic!("fixture: {error}")),
                tool: "edit".to_owned(),
                capabilities: vec![ToolCapability::FileWrite],
                detail:
                    "Change crates/plexmaton-core/src/lib.rs and preserve its current revision."
                        .to_owned(),
            },
        });
        conversation
            .state
            .set_working_directory("~/plexmaton".to_owned());
        let (surfaces, _) = draw_frame(&conversation.state, &Palette::default(), width, height);
        conversation
            .state
            .attend(&surfaces, AttentionIntent::Move(Direction::Forward));
        conversation.state.attend(&surfaces, AttentionIntent::GoTo);
        conversation.state
    }

    /// APV-4 and SURF-4: the blocking decision surface is frozen at every supported composition.
    #[test]
    fn the_approval_frames_match_their_fixtures() {
        let write = std::env::var_os("PLEXMATON_WRITE_FRAMES").is_some();
        for (name, width, height) in APPROVAL_FRAMES {
            let drawn = draw(&approval_state(width, height), width, height);
            for signature in [
                "Approval required",
                "change files",
                "Allow once",
                "> Deny",
                "Esc keeps pending",
            ] {
                assert!(
                    drawn.contains(signature),
                    "{name}: {signature:?} is not on screen"
                );
            }
            let path = fixture_path(name);
            if write {
                std::fs::write(&path, &drawn)
                    .unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
                continue;
            }
            let fixture = std::fs::read_to_string(&path).unwrap_or_else(|error| {
                panic!(
                    "read {}: {error}\nwrite the fixtures with PLEXMATON_WRITE_FRAMES=1 and review them",
                    path.display()
                )
            });
            assert!(
                fixture == drawn,
                "{name} drifted from its fixture at {}",
                first_difference(&fixture, &drawn)
            );
        }
    }

    fn native_approval_state(width: u16, height: u16) -> ViewState {
        let mut conversation = Conversation::canonical();
        conversation.emit(SessionEvent::ToolCallChanged {
            agent_id: AgentId::new("agent-a").unwrap_or_else(|error| panic!("fixture: {error}")),
            call_id: ToolCallId::new("command-native-1")
                .unwrap_or_else(|error| panic!("fixture: {error}")),
            label: "exec_command".to_owned(),
            status: ToolCallStatus::AwaitingApproval,
        });
        conversation.emit(SessionEvent::AttentionRequested {
            agent_id: AgentId::new("agent-a").unwrap_or_else(|error| panic!("fixture: {error}")),
            attention_id: AttentionId::new("attention-a-command")
                .unwrap_or_else(|error| panic!("fixture: {error}")),
            request: AttentionRequest::Approval {
                approval_id: ApprovalId::new("approval-a-command")
                    .unwrap_or_else(|error| panic!("fixture: {error}")),
                call_id: ToolCallId::new("command-native-1")
                    .unwrap_or_else(|error| panic!("fixture: {error}")),
                tool: "exec_command".to_owned(),
                capabilities: vec![
                    ToolCapability::FileRead,
                    ToolCapability::FileWrite,
                    ToolCapability::ProcessSpawn,
                ],
                detail: "Command \"cargo test\" · cwd \"/Users/ac/dev/agents/coding/plexmaton\" · timeout 120000 ms"
                    .to_owned(),
            },
        });
        conversation
            .state
            .set_working_directory("~/plexmaton".to_owned());
        let (surfaces, _) = draw_frame(&conversation.state, &Palette::default(), width, height);
        conversation
            .state
            .attend(&surfaces, AttentionIntent::Move(Direction::Forward));
        conversation.state.attend(&surfaces, AttentionIntent::GoTo);
        conversation.state
    }

    /// LIVE-1 and APV-4: the concrete native command decision remains legible at every width.
    #[test]
    fn the_native_approval_frames_match_their_fixtures() {
        let write = std::env::var_os("PLEXMATON_WRITE_FRAMES").is_some();
        for (name, width, height) in NATIVE_APPROVAL_FRAMES {
            let state = native_approval_state(width, height);
            let (surfaces, buffer) = draw_frame(&state, &Palette::default(), width, height);
            let drawn = region_text(&buffer, Rect::new(0, 0, width, height));
            let approval = surfaces
                .get(SurfaceId::Approval)
                .unwrap_or_else(|| panic!("{name}: approval surface is not registered"));
            let approval = region_text(&buffer, approval.bounds);
            for signature in [
                "Approval required",
                "exec_command",
                "read files",
                "change files",
                "run processes",
                "Allow once",
                "> Deny",
                "cargo test",
            ] {
                assert!(
                    approval.contains(signature),
                    "{name}: {signature:?} is not on the approval card"
                );
            }
            let path = fixture_path(name);
            if write {
                std::fs::write(&path, &drawn)
                    .unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
                continue;
            }
            let fixture = std::fs::read_to_string(&path).unwrap_or_else(|error| {
                panic!(
                    "read {}: {error}\nwrite the fixtures with PLEXMATON_WRITE_FRAMES=1 and review them",
                    path.display()
                )
            });
            assert!(
                fixture == drawn,
                "{name} drifted from its fixture at {}",
                first_difference(&fixture, &drawn)
            );
        }
    }

    /// APV-4: even the smallest supported terminal shows what will execute before Enter can
    /// grant it; scrolling is for the remainder, not for discovering the command exists.
    #[test]
    fn native_command_is_visible_before_decision_at_the_smallest_terminal() {
        let (width, height) = (48, 12);
        let state = native_approval_state(width, height);
        let (surfaces, buffer) = draw_frame(&state, &Palette::default(), width, height);
        let approval = surfaces
            .get(SurfaceId::Approval)
            .unwrap_or_else(|| panic!("approval surface is not registered"));
        let status = surfaces
            .get(SurfaceId::Status)
            .unwrap_or_else(|| panic!("status surface is not registered"));
        assert!(
            approval.bounds.bottom() <= status.bounds.top(),
            "the compact modal must not cover the status line"
        );
        let approval = region_text(&buffer, approval.bounds);

        for signature in ["Allow once", "> Deny", "Command \"cargo test\""] {
            assert!(
                approval.contains(signature),
                "smallest approval card hid {signature:?}:\n{approval}"
            );
        }
    }

    /// The product's own text: every cell except the two conversations' bodies, which hold what
    /// a producer said and are not the screen's copy.
    fn chrome_text(buffer: &Buffer, surfaces: &SurfaceTree) -> String {
        let mut text = String::new();
        for surface in surfaces.iter() {
            let bounds = surface.bounds;
            let region = match surface.id {
                SurfaceId::Transcript | SurfaceId::Inspector => {
                    text.push_str(&region_text(
                        buffer,
                        Rect {
                            height: 1,
                            ..bounds
                        },
                    ));
                    text.push('\n');
                    Rect {
                        y: bounds.bottom().saturating_sub(1),
                        height: 1,
                        ..bounds
                    }
                }
                _ => bounds,
            };
            text.push_str(&region_text(buffer, region));
            text.push('\n');
        }
        text
    }

    /// Phase 01 §scope 1: no user-facing word is `inspector`, `shelf` or `column`.
    ///
    /// Three states, because the second window's copy exists only while it is open, and its
    /// input only while it is entered.
    #[test]
    fn no_word_on_screen_names_a_mechanism() {
        let palette = Palette::default();
        let mut closed = canonical_state();
        closed.set_working_directory("~/plexmaton".to_owned());
        let mut open = closed.clone();
        open.move_selection(Direction::Forward);
        let mut entered = open.clone();
        let (surfaces, _) = draw_frame(&entered, &palette, 120, 40);
        entered.inspect(&surfaces, InspectorIntent::Open);

        let states: [(&str, &ViewState); 3] =
            [("closed", &closed), ("open", &open), ("entered", &entered)];
        for (label, state) in states {
            for (width, height) in [(140, 40), (120, 40), (95, 40), (60, 40), (60, 12)] {
                let (surfaces, buffer) = draw_frame(state, &palette, width, height);
                let text = chrome_text(&buffer, &surfaces).to_lowercase();
                for word in ["inspector", "shelf", "column"] {
                    assert!(
                        !text.contains(word),
                        "{label} at {width}x{height} says {word:?}:\n{text}"
                    );
                }
            }
        }
    }
}
