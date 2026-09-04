#!/usr/bin/env bash
# User-owned presentation only. Reads the Plexmaton status snapshot on stdin; never opens JSONL.
# Requires jq. A Powerline-compatible font gives the filled arrow separators their intended shape.
set -euo pipefail

values=$(jq -r '
  def clean: if . == null then "" else tostring | gsub("[\u0000-\u001f\u007f-\u009f]"; " ") end;
  [(.model.display_name // .model.id), .effort.level, (.workspace.current_dir // .cwd),
   .plexmaton.terminal.columns,
   (if .plexmaton.context.availability == "available" then .plexmaton.context.input_tokens else null end),
   .context_window.context_window_size, .context_window.used_percentage,
   .plexmaton.context.estimated_tokens,
   .context_window.total_input_tokens, .context_window.total_output_tokens,
   .cost.total_cost_usd,
   (if .context_window.current_usage != null then .context_window.current_usage |
      if .input_tokens != null and .cache_read_input_tokens != null and .cache_creation_input_tokens != null
      then (.input_tokens + .cache_read_input_tokens + .cache_creation_input_tokens) as $n |
        if $n > 0 then (.cache_read_input_tokens * 100 / $n | round) else null end
      else null end else null end),
   .plexmaton.usage.coverage] | .[] | clean
') || exit 1
fields=()
while IFS= read -r value; do fields+=("$value"); done <<< "$values"

model=${fields[0]:-}; effort=${fields[1]:-}; cwd=${fields[2]:-}
columns=${fields[3]:-80}; occupancy=${fields[4]:-}; capacity=${fields[5]:-}
pct=${fields[6]:-}; estimated=${fields[7]:-}
input=${fields[8]:-}; output=${fields[9]:-}; cost=${fields[10]:-}; cache=${fields[11]:-}
coverage=${fields[12]:-unavailable}

format_tokens() {
  awk -v n="$1" 'BEGIN { if (n >= 1000000) printf "%.1fM", n/1000000;
    else if (n >= 1000) printf "%.1fk", n/1000; else printf "%d", n }'
}
fg() { printf '\033[38;2;%sm' "$1"; }
bg() { printf '\033[48;2;%sm' "$1"; }
reset() { printf '\033[0m'; }

labels=(); colors=()
add() { labels+=("$1"); colors+=("$2"); }
if [[ -n "$model" ]]; then
  [[ -z "$effort" || "$effort" == none ]] || model="$model $effort"
  add "$model" '180;150;235'
fi
branch=""
if [[ -n "$cwd" ]]; then
  branch=$(git --no-optional-locks -C "$cwd" symbolic-ref --short HEAD 2>/dev/null \
    || git --no-optional-locks -C "$cwd" rev-parse --short HEAD 2>/dev/null || true)
fi
# Branch and path are external text too; the application independently rejects terminal controls.
branch=$(printf '%s' "$branch" | tr -d '\000-\037\177')
[[ -z "$branch" ]] || add "$branch" '140;218;165'
if [[ -n "$occupancy" && -n "$capacity" && -n "$pct" ]]; then
  approx=""; [[ -z "$estimated" || "$estimated" == 0 ]] || approx='~'
  rounded=$(awk -v n="$pct" 'BEGIN { printf "%d", n }')
  color='200;224;120'
  if (( rounded >= 80 )); then color='255;120;120';
  elif (( rounded >= 50 )); then color='255;196;102'; fi
  add "ctx ${approx}$(format_tokens "$occupancy")/$(format_tokens "$capacity") ${rounded}%" "$color"
fi
[[ -z "$cache" ]] || add "cache ${cache}%" '120;210;205'
traffic=""
[[ -z "$input" ]] || traffic="↑$(format_tokens "$input")"
[[ -z "$output" ]] || traffic="${traffic:+$traffic }↓$(format_tokens "$output")"
if [[ -n "$traffic" ]]; then
  [[ "$coverage" != partial ]] || traffic="$traffic reported"
  add "$traffic" '130;180;240'
fi
if [[ -n "$cost" ]]; then
  price=$(awk -v n="$cost" 'BEGIN { printf "$%.3f", n }')
  add "$price" '235;140;200'
fi

line_cells=0
for ((i=0; i<${#labels[@]}; i++)); do
  needed=$((${#labels[i]} + 3))
  if (( line_cells > 0 && line_cells + needed > columns )); then
    printf '\n'; line_cells=0
  fi
  bg "${colors[i]}"; fg '45;40;64'; printf '\033[1m %s ' "${labels[i]}"; reset
  fg "${colors[i]}"
  next=$((i+1))
  if (( next < ${#labels[@]} && line_cells + needed + ${#labels[next]} + 3 <= columns )); then
    bg "${colors[next]}"
  fi
  printf '\xee\x82\xb0'; reset
  line_cells=$((line_cells + needed))
done
(( ${#labels[@]} == 0 )) || printf '\n'

if [[ -n "$cwd" ]]; then
  shown=$cwd
  if [[ -n "${HOME:-}" && ( "$cwd" == "$HOME" || "$cwd" == "$HOME/"* ) ]]; then
    shown="~${cwd#"$HOME"}"
  fi
  IFS='/' read -r -a parts <<< "$shown"
  hues=('255;154;144' '255;196;102' '200;224;120' '140;218;165' '120;210;205' '130;180;240' '180;150;235' '235;140;200')
  index=0
  for ((j=0; j<${#parts[@]}; j++)); do
    part=${parts[j]}
    [[ -n "$part" ]] || continue
    if (( index > 0 )); then fg '140;130;150'; printf ' / '; fi
    fg "${hues[index % 8]}"
    if (( j + 1 == ${#parts[@]} )); then printf '\033[1m'; fi
    printf '%s' "$part"; reset
    index=$((index+1))
  done
  if (( index == 0 )); then fg '255;154;144'; printf '/'; reset; fi
  printf '\n'
fi
