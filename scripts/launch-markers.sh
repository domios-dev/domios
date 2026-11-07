#!/usr/bin/env bash
# Caller: evals/_lib/lib.sh, scripts/dev-notify.sh, scripts/qa-gates.lib.sh, build.sh
# Tooling clears or restores launch identity from tab_launch::MARKERS itself, never from its own copy.

LAUNCH_MARKERS_SOURCE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/src-tauri/crates/codeterm-util/src/tab_launch.rs"

# Prints one marker per line. Any entry that is not a literal or a same-file `pub const NAME: &str`
# fails the whole read, so a reshaped MARKERS can never yield a silently partial set.
launch_marker_names() {
  [ -f "$LAUNCH_MARKERS_SOURCE" ] || {
    echo "ERROR [launch-markers]: missing $LAUNCH_MARKERS_SOURCE" >&2
    return 3
  }
  awk '
    { sub(/\r$/, "") }
    FNR == NR {
      if ($0 ~ /^pub const [A-Z_][A-Z0-9_]*: &str = "[A-Z_][A-Z0-9_]*";$/) {
        name = $3; sub(/:$/, "", name)
        value = $0; sub(/^[^"]*"/, "", value); sub(/";$/, "", value)
        constants[name] = value
      }
      next
    }
    state == 0 && $0 == "pub const MARKERS: &[&str] = &[" { state = 1; next }
    state == 1 && $0 == "];" { state = 2; next }
    state == 1 {
      item = $0; sub(/^[ \t]+/, "", item); sub(/,[ \t]*$/, "", item)
      if (item ~ /^"[A-Z_][A-Z0-9_]*"$/) markers[++count] = substr(item, 2, length(item) - 2)
      else if (item in constants) markers[++count] = constants[item]
      else invalid = 1
    }
    END {
      if (state != 2 || invalid || count == 0) exit 3
      for (i = 1; i <= count; i++) print markers[i]
    }
  ' "$LAUNCH_MARKERS_SOURCE" "$LAUNCH_MARKERS_SOURCE" || {
    echo "ERROR [launch-markers]: cannot read tab_launch::MARKERS from $LAUNCH_MARKERS_SOURCE" >&2
    return 3
  }
}
