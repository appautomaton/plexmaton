#!/usr/bin/env bash
# Reports documents that have outgrown their layer in the context hierarchy.
#
# This never fails a build, deliberately. A budget here is not a length limit: it is a signal that
# content is sitting at the wrong load-time, and the escape hatch for each path — push it down,
# split it, or rewrite it — matters more than the number. Budgets are in bytes, because a line
# budget is satisfied by writing longer lines. They and their escape hatches are defined once, in
# .agents/README.md; this script only measures against them.
set -uo pipefail

cd "$(dirname "$0")/.."

budget_for() {
    case "$1" in
        AGENTS.md) echo 10240 ;;
        README.md) echo 4096 ;;
        .agents/README.md) echo 8192 ;;
        .agents/standards/*.md) echo 8192 ;;
        .agents/specs/*.md) echo 12288 ;;
        .agents/plans/*.md) echo 8192 ;;
        .agents/roadmap.md) echo 8192 ;;
        # Long-format documents. A phase or the interaction contract legitimately carries many
        # distinct sections, so their ceiling catches runaway growth rather than shaping structure.
        .agents/ui-ux.md) echo 32768 ;;
        .agents/phases/*.md) echo 32768 ;;
        .agents/research/*.md) echo 8192 ;;
        .agents/handoffs/*.md) echo 8192 ;;
        *) echo 0 ;;
    esac
}

over=0

while IFS= read -r file; do
    # A tracked file deleted in the working tree has nothing to measure.
    [[ -f "$file" ]] || continue
    budget=$(budget_for "$file")
    [[ "$budget" -eq 0 ]] && continue
    bytes=$(wc -c < "$file")
    if [[ "$bytes" -gt "$budget" ]]; then
        printf 'doc budget: %s is %d bytes, %d over its %d byte budget\n' \
            "$file" "$bytes" "$((bytes - budget))" "$budget" >&2
        over=$((over + 1))
    fi
done < <(git ls-files AGENTS.md README.md .agents | grep '\.md$' | sort)

if [[ "$over" -ne 0 ]]; then
    printf '\ndoc budget: %d document(s) over budget. Escape hatches are in .agents/README.md;\n' "$over" >&2
    printf 'raising a budget is the last option, not the first.\n' >&2
    exit 0
fi

echo "doc budget: every document is within its layer's budget"
