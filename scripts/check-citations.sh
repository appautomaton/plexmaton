#!/usr/bin/env bash
# Every identifier the code cites must resolve to a document that outlives the code.
#
# "Cite, don't restate" makes a bare `SURF-3` or `INS-5` in a comment or a test name carry real
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

# A contract section cited as `ui-ux §name` must be a heading in ui-ux.md.
# Only a cite closed by punctuation is checked, so prose that runs on past the name is left alone.
cited_sections=$(grep -rhoE 'ui-ux(\.md)?`? §[a-z][a-z -]*[a-z][).,;:]' --include='*.rs' --include='*.md' crates/ .agents/ AGENTS.md README.md 2>/dev/null |
    sed -E 's/.*§//; s/[).,;:]$//' | sort -u || true)
while IFS= read -r section; do
    [[ -z "$section" ]] && continue
    if ! grep -qiE "^#+ .*${section}" .agents/roadmap/ui-ux.md; then
        printf 'citation: ui-ux §%s is cited but no such heading exists\n' "$section" >&2
        fail=1
    fi
done <<< "$cited_sections"

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

count=$(printf '%s\n' "$cited_invariants" | grep -c . || true)
evidence_count=$(printf '%s\n' "$evidence" | grep -c . || true)
sections=$(printf '%s\n' "$cited_sections" | grep -c . || true)
echo "citations: ${count} invariants and ${sections} contract sections cited, ${evidence_count} named proofs; all resolve"
