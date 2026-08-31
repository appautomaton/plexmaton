#!/usr/bin/env bash
# Reports documents that have outgrown their layer in the context hierarchy.
#
# This never fails a build, deliberately. A budget here is not a length limit: it is a signal that
# content is sitting at the wrong load-time, and the escape hatch for each path — push it down,
# split it, or age it — matters more than the number. Budgets and their escape hatches are defined
# once, in .agents/README.md; this script only measures against them.
set -uo pipefail

cd "$(dirname "$0")/.."

budget_for() {
    case "$1" in
        AGENTS.md) echo 120 ;;
        .agents/README.md) echo 100 ;;
        .agents/standards/*.md) echo 150 ;;
        .agents/specs/*.md) echo 200 ;;
        .agents/plans/*.md) echo 150 ;;
        .agents/DECISIONS.md) echo 200 ;;
        .agents/roadmap/plexmaton.md) echo 250 ;;
        # Long-format documents. A phase or the interaction contract legitimately carries many
        # distinct sections, so their ceiling catches runaway growth rather than shaping structure.
        .agents/roadmap/ui-ux.md) echo 750 ;;
        .agents/roadmap/phase-*.md) echo 750 ;;
        .agents/handoffs/*.md) echo 200 ;;
        *) echo 0 ;;
    esac
}

over=0

while IFS= read -r file; do
    budget=$(budget_for "$file")
    [[ "$budget" -eq 0 ]] && continue
    lines=$(wc -l < "$file")
    if [[ "$lines" -gt "$budget" ]]; then
        printf 'doc budget: %s is %d lines, %d over its %d line budget\n' \
            "$file" "$lines" "$((lines - budget))" "$budget" >&2
        over=$((over + 1))
    fi
done < <(git ls-files AGENTS.md .agents | grep '\.md$' | sort)

if [[ "$over" -ne 0 ]]; then
    printf '\ndoc budget: %d document(s) over budget. Escape hatches are in .agents/README.md;\n' "$over" >&2
    printf 'raising a budget is the last option, not the first.\n' >&2
    exit 0
fi

echo "doc budget: every document is within its layer's budget"
