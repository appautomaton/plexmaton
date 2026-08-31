#!/usr/bin/env bash
# Sentinel against file sprawl.
#
# Rust has no community file-length standard and Clippy has no file-length lint, so this is a
# project convention rather than an industry one. The real guards are the function-level
# thresholds in clippy.toml; this only catches a module that has quietly accumulated several
# responsibilities without any single function growing.
#
# Inline `#[cfg(test)]` modules are the Rust convention, so tests must not count against the
# budget. Only the code above the first test module is measured.
set -euo pipefail

cd "$(dirname "$0")/.."

LIMIT="${FILE_LENGTH_LIMIT:-400}"
fail=0

while IFS= read -r file; do
    total=$(wc -l < "$file")
    code=$(awk '/#\[cfg\(test\)\]/{print NR-1; exit}' "$file")
    code=${code:-$total}
    if [[ "$code" -gt "$LIMIT" ]]; then
        printf '%s: %d code lines exceeds the %d line sentinel\n' "$file" "$code" "$LIMIT" >&2
        fail=1
    fi
done < <(find crates -name '*.rs' -type f | sort)

if [[ "$fail" -ne 0 ]]; then
    cat >&2 <<'HINT'

Split the file by responsibility and invariant. Raising the sentinel is not the fix: AGENTS.md
asks for cohesive modules, and this threshold exists only to make sprawl visible.
HINT
    exit 1
fi

echo "file length: every file is within the ${LIMIT} line sentinel"
