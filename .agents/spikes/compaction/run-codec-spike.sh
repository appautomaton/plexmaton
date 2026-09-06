#!/usr/bin/env bash
# Build first-party codec rlibs in this worktree, then run the isolated test spike against them.
set -euo pipefail
shopt -s nullglob

target_dir=target/compaction-spike
cargo build --offline -p plexmaton-provider --target-dir "$target_dir"

rlib() {
  local name=$1
  local candidates=("$target_dir"/debug/deps/lib"$name"-*.rlib)
  if [ "${#candidates[@]}" -ne 1 ]; then
    printf 'expected one rlib for %s; found %s\n' "$name" "${#candidates[@]}" >&2
    return 1
  fi
  printf '%s' "${candidates[0]}"
}

rustc --edition=2024 --test .agents/spikes/compaction/codec-prefix.rs \
  -L "dependency=$target_dir/debug/deps" \
  --extern "plexmaton_agent=$(rlib plexmaton_agent)" \
  --extern "plexmaton_core=$(rlib plexmaton_core)" \
  --extern "plexmaton_provider=$(rlib plexmaton_provider)" \
  --extern "serde_json=$(rlib serde_json)" \
  -o "$target_dir/codec-prefix"
"$target_dir/codec-prefix"

rustc --edition=2024 --test .agents/spikes/compaction/budget-boundary-model.rs \
  -o "$target_dir/budget-boundary-model"
"$target_dir/budget-boundary-model"
