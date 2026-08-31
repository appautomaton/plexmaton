#!/usr/bin/env bash
# Every identifier the code cites must resolve to a document that outlives the code.
#
# "Cite, don't restate" makes a bare `SURF-3` or `D-017` in a comment or a test name carry real
# weight: it is a pointer, read far from the file that defines it. A pointer to nothing is worse
# than the paraphrase it replaced, because it looks authoritative.
#
# This exists because Phase 00 produced exactly that failure. A delivery plan carried four inline
# invariants, the code cited them nine times, and the plan was deleted when its slice landed --
# which is what plans are for. Prose could not have caught it; the rule now has a gate.
set -euo pipefail

cd "$(dirname "$0")/.."

fail=0

# Rust sources only. A shell or awk fragment such as `NR-1` is identifier-shaped and is not a
# citation, and no script carries one today; widen this when one does.
#
# Spec invariants are declared as `**PREFIX-N — ...` in a specs/ file.
cited_invariants=$(grep -rhoE '\b[A-Z]{2,6}-[0-9]+\b' --include='*.rs' crates/ 2>/dev/null |
    grep -vE '^(D|UTF|SHA|RGB|ISO|HTTP)-' | sort -u || true)
for id in $cited_invariants; do
    if ! grep -rqF "**${id} " .agents/specs/; then
        printf 'citation: %s is cited in code but declared in no spec\n' "$id" >&2
        fail=1
    fi
done

# Decisions are ledger rows in DECISIONS.md.
cited_decisions=$(grep -rhoE '\bD-[0-9]{3}\b' --include='*.rs' crates/ 2>/dev/null | sort -u || true)
for id in $cited_decisions; do
    if ! grep -qE "^\| ${id} \|" .agents/DECISIONS.md; then
        printf 'citation: %s is cited in code but has no ledger row\n' "$id" >&2
        fail=1
    fi
done

if [[ "$fail" -ne 0 ]]; then
    cat >&2 <<'HINT'

A cited identifier must live somewhere that outlives the citation. If it came from a plan, the
citation is the proof that the contract is durable: promote it to .agents/specs/ rather than
deleting the citation. See .agents/plans/README.md.
HINT
    exit 1
fi

count=$(printf '%s\n%s\n' "$cited_invariants" "$cited_decisions" | grep -c . || true)
echo "citations: all ${count} identifiers cited in code resolve"
