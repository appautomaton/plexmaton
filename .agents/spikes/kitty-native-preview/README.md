# Native Kitty preview

Read when reviewing Plexmaton layout and child control in the user's actual terminal.

## Question

Can the actual Workspace renderer demonstrate child control with Kitty's native cell geometry,
font rendering, colors, input and resize behavior, independently of production activation?

## Run

From the task worktree, in Kitty:

```sh
cargo run --locked -p plexmaton-tui --example native_preview
```

The preview starts with Main's composer focused and a Main-controlled child open. On Narrow the
child owns the body and its top-right `Agents ^B` handle opens the full-region navigator by click;
`Ctrl-B` is the keyboard equivalent. `F6` advances
Main running → Main idle → Handoff pending → User idle. These are explicit fixture snapshots,
not durable operations; the final state does not cycle back to Main. Acknowledgment keeps the
current focus. `Tab`, or a click in the child's conversation, enters it; only User control allows
its input. `Escape` backs out and `Ctrl-D` twice within one second exits. Ctrl-C's Stop hint appears
only when a running child holds focus. External work is unavailable in this preview.

The roster needs several agents to say anything, and this fixture has one. For the panel's own
behaviour — ordering, the ruled break, and `Ctrl-B` — use:

```sh
cargo run --locked -p plexmaton-tui --example roster_preview
```

Five agents in four states at once: one failed, one wanting an approval, one asking a question, two
working. `Ctrl-B` puts the panel away and brings it back, `↑`/`↓` move the selection, and `Enter`
enters an agent, which for one that is asking is also going to its request. Resizing past 72 columns
moves the panel between a column and the Narrow full-region navigator.

To launch a separate review window and size it in cells from another Kitty shell:

```sh
cargo build --locked -p plexmaton-tui --example native_preview
preview_id=$(kitten @ launch --type os-window --title 'Plexmaton control preview' \
  --cwd "$PWD" "$PWD/target/debug/examples/native_preview")
kitten @ resize-os-window --match "id:$preview_id" --unit cells --width 120 --height 36
```

Use 88 and 60 for the other widths. Kitty's remote-control interface must already be available;
these commands change no Kitty config. Target only the returned window ID.
[Kitty remote control](https://sw.kovidgoyal.net/kitty/remote-control/).

## Evidence and limits

On 2026-09-13, Kitty 0.46.1 rendered the fixture using the user's existing configuration
(configured font: Agave Nerd Font Mono, 14 pt). The user confirmed that the native window was visible
and explicitly agreed that the demonstration was incomplete. Source data comes from a bounded
variant of `Scenario::canonical`; rendering and normal event routing use the real `Workspace` and
Crossterm backend. No HTML, SVG or image is substituted for the terminal cells.

Kitty's 2026-09-13 screen readback verified the control baseline at 36 rows per width; these files
do not establish the refreshed Narrow navigation:

| Width | Main running, primary input focused | User control, child explicitly entered |
| --- | --- | --- |
| 120 | [Native cells](./frames/main-running-120.txt) | [Native cells](./frames/user-entered-120.txt) |
| 88 | [Native cells](./frames/main-running-88.txt) | [Native cells](./frames/user-entered-88.txt) |
| 60 | [Native cells](./frames/main-running-60.txt) | [Native cells](./frames/user-entered-60.txt) |

[Control transition excerpts](./frames/control-transitions.txt) retain the native title and controller
rows for all four states at all three widths. The primary input retained focus through acknowledgment;
only explicit entry revealed child input. Normal exit returned code zero, restored the inherited
terminal modes exactly and released the alternate screen. The temporary local probe and structured
report are in `target/native-preview-validation/`; the retained frames above are the durable evidence.

Current validation passes all 508 TUI tests, the full workspace suite, workspace Clippy, 42 Python
tests, corpus gates, terminal lifecycle smoke and the real delegation smoke. The user reviewed the
fresh 60 × 36 Kitty preview on 2026-09-16 and accepted its structure and function; minor visual
refinements remain separate work.

[CCV-1–CCV-4](../../specs/child-control-view.md) own the tested controller/input boundary. The fixture
loads no provider configuration, makes no model requests and writes no conversation. Rejected
messages and consumed commands retain their drafts; listings receive failure states; Configuration
names the offline fixture. Ctrl-C interrupt requests, approvals, permissions and clipboard delivery
never report success. Preparation is synchronous for this finite fixture; native math output and
production responsiveness are not demonstrated.

Child history and mail are synthetic. This scenario has no Attention request. The separate real PTY
covers authenticated control snapshots, Stop/Handoff settlement, provider activation, canonical
mail and Attention. The fixture remains presentation evidence rather than an authenticated product
source.
