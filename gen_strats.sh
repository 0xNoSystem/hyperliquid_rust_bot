#!/usr/bin/env bash
set -euo pipefail

STRATS_DIR="src/strats"
OUT_FILE="web_ui/src/strats.ts"

to_camel_case() {
  local s="$1"
  local out=""
  IFS="_" read -ra parts <<< "$s"
  for i in "${!parts[@]}"; do
    if [ "$i" -eq 0 ]; then
      out+="${parts[i]}"
    else
      out+="${parts[i]^}"
    fi
  done
  echo "$out"
}

strategies=()

for f in "$STRATS_DIR"/*.rs; do
  name="$(basename "$f" .rs)"
  if [ "$name" = "mod" ]; then
    continue
  fi
  strategies+=( "$(to_camel_case "$name")" )
done

{
  echo "// AUTO-GENERATED — DO NOT EDIT"
  echo

  # union type
  echo "export type Strategy ="
  for i in "${!strategies[@]}"; do
    s="${strategies[i]}"
    if [ "$i" -eq $((${#strategies[@]} - 1)) ]; then
      echo "  | \"$s\";"
    else
      echo "  | \"$s\""
    fi
  done
  echo

  # options array
  echo "export const strategyOptions: readonly Strategy[] = ["
  for s in "${strategies[@]}"; do
    echo "  \"$s\","
  done
  echo "] as const;"
} > "$OUT_FILE"

