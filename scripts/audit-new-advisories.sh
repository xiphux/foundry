#!/usr/bin/env bash
# Fail on the RustSec findings this commit adds, not on every one in
# Cargo.lock.
#
# `cargo audit --deny warnings` on its own fails whenever the lock holds any
# vulnerability or warning, so one published against a crate already locked --
# with or without a fix -- failed every pull request until someone dealt with
# it, whatever the pull request changed. Two Renovate PRs that each fixed one
# could never merge, since each still carried the other's. A pull request can
# answer for what it changes, so that is what this asks.
#
# It audits Cargo.lock at HEAD and at HEAD's first parent and fails only on
# findings HEAD has that the parent does not. On a pull request,
# actions/checkout checks out GitHub's merge commit, whose first parent is the
# target branch, so the parent is "the base without this change"; on a push it
# is the previous commit. The checkout therefore needs `fetch-depth: 2`.
#
# A finding is a vulnerability or a warning -- unmaintained, unsound, yanked
# -- as `--deny warnings` treated them, keyed by advisory id (a yanked version,
# which has none, by crate and version). Findings already on the parent are
# printed as warnings and do not fail the run; Dependabot alerts and Renovate
# still surface them. Both locks are audited with this checkout's settings, so
# an `ignore` added to audit.toml applies to both sides.
#
# Arguments are passed to both `cargo audit` runs.
set -euo pipefail

workdir=$(mktemp -d)
trap 'rm -rf "$workdir"' EXIT

fail() {
  # Every failure fails the gate: a report that could not be produced proves
  # nothing about the lock.
  echo "::error title=Audit failed::$1"
  exit 1
}

# One line per finding: key <TAB> description.
findings() {
  local lockfile=$1 report=$workdir/report.json
  shift
  # cargo audit exits non-zero when it finds anything, so the status says
  # nothing; only output that is not a report -- the advisory database
  # unreachable, a lock it cannot parse -- is an error.
  cargo audit --json --file "$lockfile" "$@" > "$report" || true
  jq -e '.vulnerabilities and has("warnings")' "$report" > /dev/null 2>&1 ||
    fail "cargo audit produced no report for $lockfile"
  jq -r '
    def described: "\(.package.name) \(.package.version)" +
      (if .advisory then ": \(.advisory.title)" else "" end);
    (.vulnerabilities.list[] | "\(.advisory.id)\tvulnerability \(.advisory.id) in \(described)"),
    (.warnings | to_entries[] | .key as $kind | .value[] |
      if .advisory then "\(.advisory.id)\t\($kind) \(.advisory.id) in \(described)"
      else "\($kind):\(.package.name)@\(.package.version)\t\($kind) \(described)" end)
  ' "$report" | sort -u
}

findings Cargo.lock "$@" > "$workdir/head.tsv"

parent=yes
if git rev-parse --verify --quiet 'HEAD^1^{commit}' > /dev/null; then
  git show 'HEAD^1:Cargo.lock' > "$workdir/parent.lock" ||
    fail "Cargo.lock is not in HEAD's parent"
  # The advisory database was just fetched for HEAD; use the same one.
  findings "$workdir/parent.lock" --no-fetch "$@" > "$workdir/parent.tsv"
elif [ "$(git rev-parse --is-shallow-repository)" = true ]; then
  fail "HEAD's parent is not in this shallow clone: check out with \`fetch-depth: 2\`"
else
  parent=no # A root commit: there is nothing to compare against.
  : > "$workdir/parent.tsv"
fi

cut -f1 "$workdir/parent.tsv" | sort -u > "$workdir/parent.keys"
added=0 existing=0
while IFS=$'\t' read -r key description; do
  if grep -qxF -- "$key" "$workdir/parent.keys"; then
    existing=$((existing + 1))
    echo "::warning title=Existing advisory::$description -- already on the parent commit, so not failing this run"
  else
    added=$((added + 1))
    echo "::error title=New advisory::$description"
  fi
done < "$workdir/head.tsv"

note=""
[ "$parent" = no ] && note=" (no parent commit: every finding counts as new)"
echo "$added new and $existing existing finding(s)$note"
[ "$added" -eq 0 ]
