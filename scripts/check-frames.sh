#!/usr/bin/env bash
# Every checked-in frame is evidence someone can find again: cited by a document as its exact
# stem or as a `family-*` wildcard. A frame nobody names goes stale the
# moment its subject changes, and nobody notices, because nothing reads it.
#
# Documents cite what was reviewed, so they must name the stem or the family.
# Examples only generate frames; generating one does not earn it a place in the repository.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

fail=0
while IFS= read -r frame; do
    # An index entry deleted from the working tree is on its way out, not evidence.
    [[ -e "$frame" ]] || continue
    stem=$(basename "$frame")
    stem="${stem%.*}"
    if rg -q --fixed-strings "$stem" --glob '*.md' .agents README.md; then
        continue
    fi
    prefix="$stem"
    found=0
    while [[ "$prefix" == *-* ]]; do
        prefix="${prefix%-*}"
        if rg -q --fixed-strings "$prefix-*" --glob '*.md' .agents README.md; then
            found=1
            break
        fi
    done
    if [[ "$found" == 0 ]]; then
        echo "frames: no document cites $frame; cite it or delete it" >&2
        fail=1
    fi
done < <(git ls-files 'crates/*/frames/**' '.agents/spikes/*/frames/**')

if [[ "$fail" == 1 ]]; then
    exit 1
fi
echo "frames: every checked-in frame is cited by a document"
