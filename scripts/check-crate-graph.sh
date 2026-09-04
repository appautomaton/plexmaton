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

# Reads one crate's normal dependency closure, or fails the run saying why.
#
# Cargo's own errors are surfaced rather than discarded: a row naming a crate that does not exist —
# a typo, a rename, a crate that moved — would otherwise read as an empty closure, and an empty
# closure contains no forbidden dependency. A gate that passes when it cannot see is worse than no
# gate, because it is believed.
closure_of() {
    local crate="$1" output
    if ! output=$(cargo tree -p "$crate" --edges normal --prefix none 2>&1); then
        printf 'cannot read the dependency closure of %s:\n%s\n' "$crate" "$output" >&2
        fail=1
        return 1
    fi
    printf '%s\n' "$output"
}

# Usage: forbid <crate> <description> <extended-regex matched against the closure>
forbid() {
    local crate="$1" why="$2" pattern="$3" closure hit
    closure=$(closure_of "$crate") || return
    hit=$(printf '%s\n' "$closure" | awk '{print $1}' | sort -u | grep -E "^(${pattern})$" || true)
    if [[ -n "$hit" ]]; then
        printf '%s must not reach %s: %s\n' "$crate" "$why" "$(echo "$hit" | tr '\n' ' ')" >&2
        fail=1
    fi
}

# Usage: only <crate> <the workspace crates it may reach, space separated and sorted>
#
# The whole closure rather than the direct dependencies, so a workspace crate reached through
# another one is caught too.
only() {
    local crate="$1" allowed="$2" closure reached
    closure=$(closure_of "$crate") || return
    reached=$(printf '%s\n' "$closure" | awk 'NR > 1 {print $1}' |
        grep '^plexmaton-' | sort -u | tr '\n' ' ')
    reached="${reached% }"
    if [[ "$reached" != "$allowed" ]]; then
        printf '%s reaches [%s], expected [%s]\n' "$crate" "$reached" "$allowed" >&2
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
    'tokio|reqwest|hyper|crossterm|ratatui|plexmaton-agent|plexmaton-provider|plexmaton-runtime|plexmaton-sim|plexmaton-tui'

# The projection consumes events and emits intents. It calls no producer, and the phase's exit
# gate says so; this is where that sentence is enforced rather than asserted.
forbid plexmaton-tui "a producer, a runtime, or a network client" \
    'tokio|reqwest|hyper|plexmaton-agent|plexmaton-command|plexmaton-file-tools|plexmaton-provider|plexmaton-runtime|plexmaton-sim'
only plexmaton-tui "plexmaton-core"

# Wire codecs parse and encode. The live runtime owns HTTP, TLS, cancellation and task lifecycle;
# putting a client here would turn an adapter into an unobservable second runtime.
forbid plexmaton-provider "an HTTP client, runtime, or terminal" \
    'tokio|tokio-util|reqwest|hyper|h2|rustls|mio|crossterm|ratatui'
only plexmaton-provider "plexmaton-agent plexmaton-core"

# Native file tools know the loop's admission vocabulary and shared capability identities. They
# perform local bounded I/O, but do not own HTTP, async runtime tasks, terminal state, or composition.
forbid plexmaton-file-tools "a provider, runtime, network client, terminal, or composition root" \
    'tokio|tokio-util|reqwest|hyper|h2|rustls|mio|crossterm|ratatui|plexmaton-cli|plexmaton-provider|plexmaton-runtime|plexmaton-sim|plexmaton-tui'
only plexmaton-file-tools "plexmaton-agent plexmaton-core"

# Native command admission and execution use the loop's trusted-call vocabulary and an async
# process owner, but they know no provider dialect, live composition owner or presentation.
forbid plexmaton-command "a provider adapter, composition root, simulator, or terminal" \
    'reqwest|hyper|h2|rustls|crossterm|ratatui|plexmaton-cli|plexmaton-provider|plexmaton-runtime|plexmaton-sim|plexmaton-tui'
only plexmaton-command "plexmaton-agent plexmaton-core"

# The runtime is the outward dependency point: it may perform the loop's effects through the
# selected codec and native executors, but it must not reach the projection or synthetic producer.
forbid plexmaton-runtime "a terminal, projection, or synthetic producer" \
    'crossterm|ratatui|plexmaton-sim|plexmaton-tui'
only plexmaton-runtime "plexmaton-agent plexmaton-command plexmaton-core plexmaton-file-tools plexmaton-provider"

# The session store performs bounded local persistence over the canonical agent journal. It owns
# no async runtime, provider transport, terminal state, tool executor, or composition root.
forbid plexmaton-session-store "a runtime, provider, tool executor, terminal, or composition root" \
    'tokio|tokio-util|reqwest|hyper|h2|rustls|mio|crossterm|ratatui|plexmaton-cli|plexmaton-command|plexmaton-file-tools|plexmaton-provider|plexmaton-runtime|plexmaton-sim|plexmaton-tui'
only plexmaton-session-store "plexmaton-agent plexmaton-core"

if [[ "$fail" -ne 0 ]]; then
    cat >&2 <<'HINT'

The arrow is the design. If the dependency is genuinely needed, the thing that needs it belongs in
a crate that is allowed to have it — usually the composition root, which is the only crate that
knows every other one exists.
HINT
    exit 1
fi

echo "crate graph: every forbidden arrow is still absent"
