#!/usr/bin/env bash
# The dependency arrows, checked instead of reviewed.
#
# Inside a crate every module can reach every other one, so "the loop must not touch the network"
# is a convention. Across a crate boundary it is a build failure, and this script is what turns the
# boundaries we chose into a gate: each row names a crate and what must not appear anywhere in its
# normal dependency closure. Development dependencies are excluded on purpose — a test harness
# reaching for the simulator says nothing about what ships.
#
# Adding a row is cheap. Deleting one to make a commit pass is the defect the row existed to catch.
set -euo pipefail

cd "$(dirname "$0")/.."

fail=0

# Usage: forbid <crate> <description> <extended-regex matched against the closure>
forbid() {
    local crate="$1" why="$2" pattern="$3" hit
    hit=$(cargo tree -p "$crate" --edges normal --prefix none 2>/dev/null |
        awk '{print $1}' | sort -u | grep -E "^(${pattern})$" || true)
    if [[ -n "$hit" ]]; then
        printf '%s must not reach %s: %s\n' "$crate" "$why" "$(echo "$hit" | tr '\n' ' ')" >&2
        fail=1
    fi
}

# Usage: only <crate> <space separated workspace crates it may depend on directly>
only() {
    local crate="$1" allowed="$2" direct
    direct=$(cargo tree -p "$crate" --edges normal --depth 1 --prefix none 2>/dev/null |
        awk 'NR > 1 {print $1}' | grep '^plexmaton-' | sort -u | tr '\n' ' ')
    direct="${direct% }"
    if [[ "$direct" != "$allowed" ]]; then
        printf '%s depends on [%s], expected [%s]\n' "$crate" "$direct" "$allowed" >&2
        fail=1
    fi
}

# The loop decides; it never performs. An effect is a value it returns, so a runtime, a client or
# a terminal in this closure means the decision and the doing have been put back together.
forbid plexmaton-agent "a runtime, a network client, or a terminal" \
    'tokio|tokio-util|reqwest|hyper|h2|rustls|mio|crossterm|ratatui'
only plexmaton-agent "plexmaton-core"

# The shared vocabulary answers to both sides, so it may not carry either side's machinery.
forbid plexmaton-core "either side's machinery" \
    'tokio|reqwest|hyper|crossterm|ratatui|plexmaton-agent|plexmaton-sim|plexmaton-tui'

# The projection consumes events and emits intents. It calls no producer, and the phase's exit
# gate says so; this is where that sentence is enforced rather than asserted.
forbid plexmaton-tui "a producer, a runtime, or a network client" \
    'tokio|reqwest|hyper|plexmaton-agent|plexmaton-sim'
only plexmaton-tui "plexmaton-core"

if [[ "$fail" -ne 0 ]]; then
    cat >&2 <<'HINT'

The arrow is the design. If the dependency is genuinely needed, the thing that needs it belongs in
a crate that is allowed to have it — usually the composition root, which is the only crate that
knows every other one exists.
HINT
    exit 1
fi

echo "crate graph: every forbidden arrow is still absent"
