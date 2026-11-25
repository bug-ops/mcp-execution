#!/usr/bin/env bash
# Print the body of one CHANGELOG.md section: the lines between its "## [<version>]"
# heading and the next "## " heading, without leading or trailing blank lines.
# Exits 1 when the section is missing or has no content.
#
# Usage: changelog-section.sh <version|Unreleased> [changelog-path]
set -euo pipefail

if [[ $# -lt 1 || $# -gt 2 ]]; then
  echo "usage: $0 <version|Unreleased> [changelog-path]" >&2
  exit 2
fi

version="$1"
changelog="${2:-CHANGELOG.md}"

awk -v version="$version" '
  /^## / {
    if (in_section) { exit }
    in_section = (index($0, "## [" version "]") == 1)
    next
  }
  in_section {
    if ($0 ~ /^[[:space:]]*$/) { blanks++; next }
    if (printed) { for (i = 0; i < blanks; i++) print "" }
    blanks = 0
    printed = 1
    print
  }
  END { exit printed ? 0 : 1 }
' "$changelog" || {
  echo "error: no content for section [$version] in $changelog" >&2
  exit 1
}
