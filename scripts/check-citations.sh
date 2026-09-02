#!/usr/bin/env bash
# Every identifier the code cites must resolve to a document that outlives the code.
#
# "Cite, don't restate" makes a bare `SURF-3` or `D-017` in a comment or a test name carry real
# weight: it is a pointer, read far from the file that defines it. A pointer to nothing is worse
# than the paraphrase it replaced, because it looks authoritative.
#
# This exists because the project produced exactly that failure once. A delivery plan carried four inline
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

# Decisions are entries in DECISIONS.md, one per bold heading.
cited_decisions=$(grep -rhoE '\bD-[0-9]{3}\b' --include='*.rs' crates/ 2>/dev/null | sort -u || true)
for id in $cited_decisions; do
    if ! grep -qE "^\*\*${id} " .agents/DECISIONS.md; then
        printf 'citation: %s is cited in code but has no entry in DECISIONS.md\n' "$id" >&2
        fail=1
    fi
done

# The other direction: an evidence table names the test that proves an invariant, and a renamed test
# leaves the spec asserting something no longer checked. Only Evidence rows are scanned, because
# prose legitimately names functions that are not tests.
evidence=$(awk '/^## Evidence/{inside=1; next} /^## /{inside=0} inside && /^\| [A-Z]+-[0-9]+ \|/' \
    .agents/specs/*.md | grep -ohE '`[a-z][a-z0-9_]+`' | tr -d '`' | sort -u || true)
for name in $evidence; do
    if ! grep -rqE "fn ${name}\\(" --include='*.rs' crates/; then
        printf 'evidence: %s is named as proof but no such test exists\n' "$name" >&2
        fail=1
    fi
done

if [[ "$fail" -ne 0 ]]; then
    cat >&2 <<'HINT'

A cited identifier must live somewhere that outlives the citation. If it came from a plan, the
citation is the proof that the contract is durable: promote it to .agents/specs/ rather than
deleting the citation. See .agents/README.md §plans.
HINT
    exit 1
fi

count=$(printf '%s\n%s\n' "$cited_invariants" "$cited_decisions" | grep -c . || true)
evidence_count=$(printf '%s\n' "$evidence" | grep -c . || true)
echo "citations: ${count} identifiers cited in code and ${evidence_count} named proofs all resolve"
