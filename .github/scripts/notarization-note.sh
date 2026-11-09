#!/usr/bin/env bash
# The notarization note in a release's notes, between <!-- notarization:<state>[:<id>] --> and
# <!-- /notarization -->. Staple finds releases still waiting on Apple by the pending marker.
#
#   notarization-note.sh print <state> [submission-id]       prints the note
#   notarization-note.sh set <tag> <state> <submission-id>   replaces the note in <tag>'s release
#
# States: pending, accepted, invalid, unsigned.
set -euo pipefail

# The backticks are Markdown, not a command.
# shellcheck disable=SC2016
OPEN_ANYWAY='open System Settings → Privacy & Security and click **Open Anyway**, or run `xattr -dr com.apple.quarantine /Applications/Runecast.app`'

print() {
  local state=$1 id=${2:-}
  echo "<!-- notarization:$state${id:+:$id} -->"
  case $state in
    pending)
      echo "> [!NOTE]"
      echo "> Apple hasn't finished notarizing this build, so macOS warns the first time you open it: $OPEN_ANYWAY."
      echo "> The downloads are replaced with notarized ones once Apple accepts it, and copies you already have open normally from then on."
      ;;
    accepted) echo "Signed with a Developer ID and notarized by Apple." ;;
    invalid)
      echo "> [!WARNING]"
      echo "> Apple rejected this build's notarization, so macOS warns the first time you open it: $OPEN_ANYWAY."
      ;;
    unsigned) echo "This build isn't signed with a Developer ID, so macOS warns the first time you open it: $OPEN_ANYWAY." ;;
    *)
      echo "Unknown state: $state" >&2
      return 1
      ;;
  esac
  echo "<!-- /notarization -->"
}

set_note() {
  local tag=$1 note notes
  note=$(print "$2" "$3")
  notes=$(mktemp)
  gh release view "$tag" --json body --jq .body |
    python3 -c 'import re, sys; print(re.sub(r"<!-- notarization:.*?<!-- /notarization -->", lambda _: sys.argv[1], sys.stdin.read(), count=1, flags=re.S), end="")' "$note" > "$notes"
  gh release edit "$tag" --notes-file "$notes"
  rm "$notes"
}

case ${1:-} in
  print) print "$2" "${3:-}" ;;
  set) set_note "$2" "$3" "$4" ;;
  *)
    sed -n '2,8p' "$0" >&2
    exit 2
    ;;
esac
