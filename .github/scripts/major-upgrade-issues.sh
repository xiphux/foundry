#!/usr/bin/env bash
# Keeps one open GitHub issue per direct dependency with a breaking upgrade
# available, and closes it once the upgrade has landed.
#
#   .github/scripts/major-upgrade-issues.sh [--dry-run]
#
# Dependabot is configured to ignore majors, and has to be: it proposes only
# a crate's newest version, so a pending major would replace the minor and
# patch updates that auto-merge. This is how majors are noticed instead — an
# issue to act on, not a PR that blocks the others.
#
# "Breaking" is Cargo's compatibility rule, not the first number: 1.x -> 2.x,
# but also 0.11 -> 0.12 and 0.0.3 -> 0.0.4, since a caret requirement accepts
# none of those. Locked versions come from `cargo metadata` (the root
# package's resolved dependencies, all targets and kinds), latest stable ones
# from the crates.io API. Only crates.io dependencies are checked.
#
# Issues are matched by a marker comment in the body, so a title edited by
# hand is kept until a newer release changes the issue.
set -euo pipefail

LABEL=major-upgrade
dry_run=no
[ "${1:-}" = --dry-run ] && dry_run=yes

# Crates whose breaking upgrade is deliberately not taken, with why, as
# "name|reason" lines. Listed rather than hidden, so the reason is next to it.
SKIP=()

# The part of a version a caret requirement pins: the major, or for 0.x the
# first non-zero component with the zeros before it (1.4.2 -> 1,
# 0.11.3 -> 0.11, 0.0.4 -> 0.0.4).
compat() {
  local v=${1%%[-+]*} maj min pat
  IFS=. read -r maj min pat <<<"$v"
  if [ "$maj" != 0 ]; then echo "$maj"
  elif [ "${min:-0}" != 0 ]; then echo "0.$min"
  else echo "0.0.${pat:-0}"
  fi
}

skip_reason() {
  local entry
  for entry in ${SKIP[@]+"${SKIP[@]}"}; do
    [ "${entry%%|*}" = "$1" ] && { echo "${entry#*|}"; return; }
  done
  return 0
}

# "name version" for each direct dependency from crates.io.
locked=$(cargo metadata --format-version 1 --locked | jq -r '
  .resolve.root as $root
  | [.resolve.nodes[] | select(.id == $root) | .deps[].pkg] as $direct
  | .packages[]
  | select(.id as $id | $direct | index($id))
  | select((.source // "") | startswith("registry+https://github.com/rust-lang/crates.io-index"))
  | "\(.name) \(.version)"' | sort -u)
if [ -z "$locked" ]; then
  echo "::error::cargo metadata listed no crates.io dependencies"
  exit 1
fi

# One directory per pending upgrade, holding its issue title and body. Files
# rather than associative arrays so this also runs on macOS's bash 3.
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

while read -r name current; do
  # crates.io asks API clients to identify themselves, and refuses requests
  # without a User-Agent. A failed lookup fails the run: treating it as "up
  # to date" would close that crate's issue.
  latest=$(curl -fsS -A 'foundry major-upgrade-check (https://github.com/xiphux/foundry)' \
    "https://crates.io/api/v1/crates/$name" | jq -r '.crate.max_stable_version // empty')
  if [ -z "$latest" ]; then
    echo "::error::No stable version of $name on crates.io"
    exit 1
  fi
  from=$(compat "$current")
  to=$(compat "$latest")
  if [ "$from" = "$to" ]; then
    echo "$name: $current, latest $latest - current"
    continue
  fi
  # A locked version ahead of the latest stable (a pre-release) is not an
  # upgrade. sort -V orders the two; the newer must be the latest.
  if [ "$(printf '%s\n%s\n' "$current" "$latest" | sort -V | tail -1)" != "$latest" ]; then
    echo "$name: $current is ahead of latest stable $latest - skipping"
    continue
  fi
  reason=$(skip_reason "$name")
  if [ -n "$reason" ]; then
    echo "$name: $current -> $latest skipped: $reason"
    continue
  fi
  echo "$name: $current -> $latest - breaking upgrade available"
  mkdir "$work/$name"
  printf '%s' "Major upgrade available: $name $from → $to" >"$work/$name/title"
  cat >"$work/$name/body" <<EOF
<!-- $LABEL:$name -->
A new breaking version is out. Dependabot does not propose majors here (see
\`.github/dependabot.yml\`), so this issue stands in for the PR.

| Crate | Locked | Latest |
|---|---|---|
| [\`$name\`](https://crates.io/crates/$name/versions) | $current | $latest |

Read the release notes for breaking changes, upgrade on a branch, and let CI
judge it. This issue closes itself once \`Cargo.lock\` is on the latest
release line; the weekly check (\`.github/workflows/major-upgrade-check.yml\`)
keeps it current until then.
EOF
done <<<"$locked"

if [ "$dry_run" = yes ]; then
  for dir in "$work"/*/; do
    [ -d "$dir" ] && echo "would open or update: $(cat "$dir/title")"
  done
  exit 0
fi

gh label create "$LABEL" --force --color d4c5f9 --description 'A major version upgrade is available' >/dev/null

# "number<TAB>key" for each open issue carrying a marker. Read in full before
# anything below edits or closes an issue.
open=$(gh issue list --label "$LABEL" --state open --limit 200 --json number,body \
  --jq ".[] | [.number, (.body | capture(\"<!-- $LABEL:(?<k>[^ ]+) -->\").k // \"\")] | @tsv")

open_number() { printf '%s\n' "$open" | awk -F'\t' -v k="$1" '$2 == k { print $1; exit }'; }

for dir in "$work"/*/; do
  [ -d "$dir" ] || continue
  name=$(basename "$dir")
  title=$(cat "$dir/title")
  number=$(open_number "$name")
  if [ -z "$number" ]; then
    gh issue create --title "$title" --body-file "$dir/body" --label "$LABEL" >/dev/null
    echo "Opened: $title"
  # GitHub can hand a body back with CRLF line endings; that, and $(...)
  # dropping trailing newlines, must not count as a change, or every run
  # would rewrite every issue.
  elif [ "$(gh issue view "$number" --json body --jq .body | tr -d '\r')" != "$(cat "$dir/body")" ]; then
    # The title is only rewritten along with a body change (a newer release),
    # so a title someone edited stays put until there is news.
    gh issue edit "$number" --title "$title" --body-file "$dir/body" >/dev/null
    echo "Updated #$number: $title"
  else
    echo "Unchanged #$number: $title"
  fi
done

printf '%s\n' "$open" | while IFS=$'\t' read -r number key; do
  [ -n "$key" ] || continue
  [ -d "$work/$key" ] && continue
  # Not "caught up": the crate may instead have been removed or added to SKIP.
  gh issue close "$number" --comment 'Closing: no breaking upgrade is pending for this crate any more.' >/dev/null
  echo "Closed #$number: $key"
done
