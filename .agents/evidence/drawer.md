# Evidence — Drawer

What proves [drawer](../specs/drawer.md)'s invariants. Named functions only: the gate
resolves every one against the source, so a renamed or deleted test fails the build here.


| Invariant | Proven by |
| --- | --- |
| DRW-1 | `the_chord_pulls_the_drawer_open_and_takes_the_keyboard`, `the_drawer_opens_over_a_waiting_approval_and_leaves_the_card_alone`, `typing_filters_the_list_and_never_reaches_the_composer`, `a_page_is_found_by_its_name_and_a_slash_is_a_character`; `scripts/smoke-tui.py` pulls it open and closes it in a real terminal |
| DRW-2 | `the_drawer_spans_the_width_and_keeps_its_top_edge`, `the_drawer_spans_every_width_with_one_row_per_page`, `short_drawer_and_wheel_use_the_visible_choice_window`, `the_drawer_frames_match_their_fixtures` with the `drawer-*` frames, `the_drawer_filter_paints_its_own_caret_while_editing`, `a_long_drawer_filter_keeps_the_caret_inside_its_row` |
| DRW-3 | `drawer_retract_is_visible_on_pages_and_requires_an_unchanged_click`, `drawer_hover_and_arrows_share_one_choice_without_opening_pages`; `the_list_finds_a_page_by_name_and_hands_it_to_the_root`, `the_escape_ladder_returns_page_then_list_then_origin`, `escape_closes_the_drawer_and_returns_the_keyboard`, `stepping_stops_at_the_ends_of_the_pages`, `a_filter_matching_nothing_leaves_no_choice`, `the_chosen_drawer_row_carries_the_chosen_role_across_its_width`; `scripts/smoke-permissions.py` walks the ladder from the Permissions page |
| DRW-4 | `the_configuration_page_shows_the_resolved_model`, `short_configuration_pages_scroll_to_the_remaining_values`, `the_configuration_page_frame_matches_its_fixture` with the `drawer-configuration` frame |

## Rendered controls

Shared choice and the retract control were inspected at
[120](../../crates/plexmaton-tui/frames/interaction/drawer-120.svg),
[88](../../crates/plexmaton-tui/frames/interaction/drawer-88.svg) and
[60](../../crates/plexmaton-tui/frames/interaction/drawer-60.svg) columns;
Configuration's hovered control is
[120](../../crates/plexmaton-tui/frames/interaction/configuration-120.svg),
[88](../../crates/plexmaton-tui/frames/interaction/configuration-88.svg),
[60](../../crates/plexmaton-tui/frames/interaction/configuration-60.svg).
The `interaction_preview` example reproduces these frames.
