#!/usr/bin/env bash
# Conventional Commits (https://www.conventionalcommits.org/en/v1.0.0/) for Runecast's workflows.
#
#   changelog.sh lint <subject>...      fails if any subject isn't a conventional commit subject
#   changelog.sh next-version <bump>    the version after the last release on HEAD; bump is
#                                       auto (from the commits), patch, minor or major
#   changelog.sh notes <tag>            release notes for <tag>: its commits since the release before
#
# Releases are the vX.Y.Z tags. Before 1.0, a breaking change bumps the minor version, as feat does.
set -euo pipefail

TYPES='feat|fix|perf|refactor|docs|test|build|ci|chore|style|revert'
SUBJECT="^($TYPES)(\([a-z0-9._/-]+\))?(!)?: .+"

# The newest vX.Y.Z tag reachable from $1; nothing before the first release.
last_release() {
  git tag --merged "$1" --list 'v*' | grep -E '^v[0-9]+\.[0-9]+\.[0-9]+$' | sort -V | tail -1 || true
}

# Commits after the last release before $1, oldest first, without merges.
commits() {
  local from
  from=$(last_release "$2")
  git rev-list --no-merges --reverse "${from:+$from..}$1"
}

breaking() {
  [[ $1 =~ $SUBJECT && -n ${BASH_REMATCH[3]} ]] || grep -qE '^BREAKING[ -]CHANGE: ' <<<"$2"
}

lint() {
  local bad=0 subject
  for subject in "$@"; do
    if [[ ! $subject =~ $SUBJECT ]]; then
      echo "::error::Not a conventional commit subject: $subject"
      bad=1
    fi
  done
  if ((bad)); then
    echo "Write subjects as <type>(<optional scope>): <description>, with type one of ${TYPES//|/, }."
    echo "Add ! before the colon for a breaking change. See https://www.conventionalcommits.org/en/v1.0.0/"
  fi
  return $bad
}

next_version() {
  local bump=$1 from level=0 sha subject major minor patch
  from=$(last_release HEAD)
  if [[ -n $from && $(git rev-list --count "$from..HEAD") == 0 ]]; then
    echo "Nothing new since $from." >&2
    return 1
  fi
  if [[ $bump == auto ]]; then
    for sha in $(commits HEAD HEAD); do
      subject=$(git log -1 --format=%s "$sha")
      if breaking "$subject" "$(git log -1 --format=%b "$sha")"; then
        level=3
      elif [[ $subject =~ ^feat[\(!:] ]] && ((level < 2)); then
        level=2
      elif [[ $subject =~ ^(fix|perf)[\(!:] ]] && ((level < 1)); then
        level=1
      fi
    done
    case $level in
      3) bump="major" ;;
      2) bump="minor" ;;
      1) bump="patch" ;;
      *)
        echo "No feat, fix, perf or breaking commits since ${from:-the start}; pick patch, minor or major to release anyway." >&2
        return 1
        ;;
    esac
  fi
  IFS=. read -r major minor patch <<<"${from:-v0.0.0}"
  major=${major#v}
  if [[ $bump == major && $major == 0 ]]; then
    bump=minor
  fi
  case $bump in
    major) echo "$((major + 1)).0.0" ;;
    minor) echo "$major.$((minor + 1)).0" ;;
    patch) echo "$major.$minor.$((patch + 1))" ;;
    *)
      echo "Unknown bump: $bump" >&2
      return 1
      ;;
  esac
}

notes() {
  local tag=$1 sha subject body line desc scope
  local -a broken=() features=() fixes=() other=()
  for sha in $(commits "$tag" "$tag^"); do
    subject=$(git log -1 --format=%s "$sha")
    body=$(git log -1 --format=%b "$sha")
    if [[ $subject =~ $SUBJECT ]]; then
      scope=${BASH_REMATCH[2]}
      scope=${scope#(}
      scope=${scope%)}
      desc=${subject#*: }
      line="- ${scope:+**$scope:** }$desc (${sha:0:7})"
      if breaking "$subject" "$body"; then
        broken+=("$line")
        continue
      fi
      case ${BASH_REMATCH[1]} in
        feat) features+=("$line") ;;
        fix | perf) fixes+=("$line") ;;
        revert) other+=("$line") ;;
      esac
    else
      # From before the switch to conventional commits.
      other+=("- $subject (${sha:0:7})")
    fi
  done
  section "⚠️ Breaking changes" "${broken[@]}"
  section "Features" "${features[@]}"
  section "Fixes" "${fixes[@]}"
  section "Other changes" "${other[@]}"
}

section() {
  local title=$1
  shift
  if (($#)); then
    printf '### %s\n\n' "$title"
    printf '%s\n' "$@"
    echo
  fi
}

command=${1:-}
shift || true
case $command in
  lint) lint "$@" ;;
  next-version) next_version "${1:-auto}" ;;
  notes) notes "$1" ;;
  *)
    sed -n '2,9p' "$0" >&2
    exit 2
    ;;
esac
