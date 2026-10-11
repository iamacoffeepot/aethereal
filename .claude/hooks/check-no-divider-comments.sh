#!/usr/bin/env bash
# PreToolUse hook for Edit and Write. Refuses a change that adds a divider
# comment to a source file: a `//` or `#` comment that opens with three or more
# of `-`, `=` or `*`. Dividers the change leaves in place do not count.

set -euo pipefail

input=$(cat)
path=$(jq -r '.tool_input.file_path // ""' <<<"$input")

case "$path" in
    *.rs | *.sh | *.py | *.js | *.ts) ;;
    *) exit 0 ;;
esac

dividers() {
    grep -cE '^[[:space:]]*(//|#)[[:space:]]*[-=*]{3,}' || true
}

# An Edit replaces `old_string` with `new_string`; a Write replaces the file.
after=$(jq -r '.tool_input | .new_string // .content // ""' <<<"$input" | dividers)
if [[ $(jq -r '.tool_name' <<<"$input") == Write && -f "$path" ]]; then
    before=$(dividers <"$path")
else
    before=$(jq -r '.tool_input.old_string // ""' <<<"$input" | dividers)
fi

if ((after > before)); then
    echo "This change adds a divider comment (\`// ----\`). Use a plain comment, or split the file into modules." >&2
    exit 2
fi
