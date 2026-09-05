#!/usr/bin/env bash
# Sentinel against file sprawl.
#
# Rust has no community file-length standard and Clippy has no file-length lint, so this is a
# project convention rather than an industry one. The real guards are the function-level
# thresholds in clippy.toml; this only catches a module that has quietly accumulated several
# responsibilities without any single function growing.
#
# Tests do not count against the budget. Test-only files follow the workspace's `tests/`,
# `tests.rs`, `*_tests.rs`, or `test_support.rs` naming conventions; in mixed modules only the code
# above the first inline `#[cfg(test)]` module is measured.
set -euo pipefail

cd "$(dirname "$0")/.."

LIMIT="${FILE_LENGTH_LIMIT:-550}"
fail=0
# Capture discovery outside process substitution so an unreadable/missing source tree fails.
source_files=$(rg --files crates -g '*.rs' | sort)

while IFS= read -r file; do
    case "$file" in
        */tests/*.rs | */tests.rs | *_tests.rs | */test_support.rs)
            continue
            ;;
    esac

    # Only a trailing inline test module ends the source section. External test modules and
    # test-only helper methods must not hide production that follows them. This is the
    # rustfmt-shaped module convention, not a Rust parser; helpers count conservatively.
    code=$(awk '
        /^[[:space:]]*#\[cfg\(test\)\][[:space:]]*mod[[:space:]]+[[:alnum:]_]+[[:space:]]*\{/ {
            print NR-1; found=1; exit
        }
        /^[[:space:]]*#\[cfg\(test\)\][[:space:]]*$/ { attribute=NR; next }
        attribute && /^[[:space:]]*mod[[:space:]]+[[:alnum:]_]+[[:space:]]*\{/ {
            print attribute-1; found=1; exit
        }
        { attribute=0 }
        END { if (!found) print NR }
    ' "$file")
    if [[ "$code" -gt "$LIMIT" ]]; then
        printf '%s: %d code lines exceeds the %d line sentinel\n' "$file" "$code" "$LIMIT" >&2
        fail=1
    fi
done <<< "$source_files"

if [[ "$fail" -ne 0 ]]; then
    cat >&2 <<'HINT'

Split the file by responsibility and invariant. Raising the sentinel is not the fix: AGENTS.md
asks for cohesive modules, and this threshold exists only to make sprawl visible.
HINT
    exit 1
fi

echo "file length: every file is within the ${LIMIT} line sentinel"
