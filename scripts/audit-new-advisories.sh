#!/usr/bin/env bash
# Fail on the RustSec findings this change adds, not on every one in
# Cargo.lock.
#
# `cargo audit --deny warnings` on its own fails whenever the lock holds any
# vulnerability or warning, so one published against a crate already locked --
# with or without a fix -- failed every pull request until someone dealt with
# it, whatever the pull request changed. Two Renovate PRs that each fixed one
# could never merge, since each still carried the other's. A pull request can
# answer for what it changes, so that is what this asks.
#
# It audits Cargo.lock at HEAD and at a baseline commit and fails only on
# findings HEAD has that the baseline does not. The baseline is $AUDIT_BASE,
# which CI sets from scripts/audit-baseline.sh: the last commit a successful
# CI run passed on the target branch, on a pull request as well as a push, so
# a push of several commits, a run that failed and was followed by another, or
# a pull request onto a tip that failed, cannot pass a finding nothing
# compared against a state without it. Unset (a local run), the baseline is
# HEAD's first parent -- outside CI only: in CI an unset $AUDIT_BASE means the
# step lost it, and fails rather than quietly comparing against the parent.
# Set but empty, there is no baseline and every finding counts as new.
#
# A finding is a vulnerability or a warning -- unmaintained, unsound, yanked
# -- as `--deny warnings` treated them, keyed by advisory id (a yanked version,
# which has none, by crate and version). Findings already on the baseline are
# printed as warnings and do not fail the run -- unless HEAD has the advisory
# in a crate it did not reach on the baseline, or in a version of the crate
# older than every version the baseline had: a fixed copy swapped for an
# older vulnerable one that something else brought in. Fixes move versions
# up, so moving a copy to a newer, still-affected version is not new, nor is
# one dependent moving while another stays (a partial fix); a newer
# vulnerable copy brought in by a new dependency is the case this lets
# through. Dependabot alerts and Renovate raise vulnerabilities that have a
# fix; nothing else revisits unmaintained or
# unsound crates already locked, so read those warnings. Both locks are
# audited with this checkout's settings, so an `ignore` added to audit.toml
# applies to both sides -- and removing one that is still needed only warns.
#
# The yanked check reads the crates.io index, and loses it two ways without
# failing, as `--deny warnings` did too. A crate it cannot look up is
# reported on stderr -- "couldn't check if the package is yanked" -- and
# skipped; that message fails the run here. And an index it cannot open at
# all is reported nowhere under --json, and the whole check is skipped; so a
# canary lock holding a version known to be yanked is audited too, and the
# run fails unless that yank is reported. The audit job builds nothing, so
# the index is usually fetched fresh, and an outage there would otherwise
# pass silently.
#
# Both locks are audited with a fresh advisory database. --no-fetch on the
# baseline's would save the fetch, but in cargo-audit 0.22 it also confines
# the yank check to the index entries already cached -- HEAD's crates only
# -- so every crate the change removed would fail to look up.
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

# Writes one line per finding to $2 -- key <TAB> crate <TAB> version <TAB>
# description -- for the lock at $1. The rest of the arguments go to cargo
# audit.
findings() {
  local lockfile=$1 out=$2 report=$workdir/report.json errors=$workdir/stderr
  shift 2
  # cargo audit exits non-zero when it finds anything, so the status says
  # nothing; only output that is not a report -- the advisory database
  # unreachable, a lock it cannot parse -- is an error. Its stderr is kept
  # for the yank check below, and passed on.
  cargo audit --json --file "$lockfile" "$@" > "$report" 2> "$errors" || true
  cat "$errors" >&2
  jq -e '.vulnerabilities and has("warnings")' "$report" > /dev/null 2>&1 ||
    fail "cargo audit produced no report for $lockfile"
  if grep -q "couldn't check if the package is yanked" "$errors"; then
    fail "cargo audit could not check $lockfile for yanked crates (see above)"
  fi
  jq -r '
    def described: "\(.package.name) \(.package.version)" +
      (if .advisory then ": \(.advisory.title)" else "" end);
    def line($key; $text): "\($key)\t\(.package.name)\t\(.package.version)\t\($text)";
    (.vulnerabilities.list[] | line(.advisory.id; "vulnerability \(.advisory.id) in \(described)")),
    (.warnings | to_entries[] | .key as $kind | .value[] |
      if .advisory then line(.advisory.id; "\($kind) \(.advisory.id) in \(described)")
      else line("\($kind):\(.package.name)@\(.package.version)"; "\($kind) \(described)") end)
  ' "$report" | sort -u > "$out"
}

findings Cargo.lock "$workdir/head.tsv" "$@"

# The canary: libc 0.2.165 is yanked, so a working yank check reports it.
cat > "$workdir/canary.lock" << 'LOCK'
version = 4

[[package]]
name = "libc"
version = "0.2.165"
source = "registry+https://github.com/rust-lang/crates.io-index"
LOCK
cargo audit --json --file "$workdir/canary.lock" "$@" > "$workdir/canary.json" 2> /dev/null || true
jq -e '[.warnings.yanked[]? | select(.package.name == "libc" and .package.version == "0.2.165")] | length > 0' \
  "$workdir/canary.json" > /dev/null 2>&1 ||
  fail "cargo audit did not report libc 0.2.165 as yanked, so its yank check is not working and its report proves nothing about yanks"

if [ -n "${AUDIT_BASE+set}" ]; then
  base=$AUDIT_BASE
elif [ "${GITHUB_ACTIONS:-}" = true ]; then
  # In CI the baseline comes from scripts/audit-baseline.sh, always, even
  # when it is empty. Unset there means the step lost it -- a rename, a
  # dropped `env:` -- and the parent is the comparison that let a push of
  # several commits through, so refuse rather than fall back to it.
  fail "AUDIT_BASE is not set: in CI it must come from scripts/audit-baseline.sh"
elif base=$(git rev-parse --verify --quiet 'HEAD^1^{commit}'); then
  :
elif [ "$(git rev-parse --is-shallow-repository)" = true ]; then
  fail "HEAD's parent is not in this shallow clone: check out with more history"
else
  base="" # A root commit: there is nothing to compare against.
fi

note=""
if [ -n "$base" ]; then
  git rev-parse --verify --quiet "$base^{commit}" > /dev/null ||
    fail "the baseline commit $base is not in this clone"
  git show "$base:Cargo.lock" > "$workdir/base.lock" ||
    fail "Cargo.lock is not in the baseline commit $base"
  findings "$workdir/base.lock" "$workdir/base.tsv" "$@"
  note=" against ${base:0:12}"
else
  : > "$workdir/base.tsv"
  note=" (no baseline commit: every finding counts as new)"
fi

# Each finding is new if the baseline did not have its key, or had it but
# not in this crate, or only in versions all newer than this one; otherwise
# existing. (No apostrophes in the program: it is single-quoted.)
awk -F'\t' -v OFS='\t' '
  # Whether version a precedes version b. One that is not semver counts as
  # older: it cannot be shown to be the newer copy a fix arrives as.
  function older(a, b,   x, y, ap, bp, i) {
    sub(/\+.*/, "", a); sub(/\+.*/, "", b)
    ap = ""; bp = ""
    if (index(a, "-")) { ap = substr(a, index(a, "-") + 1); a = substr(a, 1, index(a, "-") - 1) }
    if (index(b, "-")) { bp = substr(b, index(b, "-") + 1); b = substr(b, 1, index(b, "-") - 1) }
    if (split(a, x, ".") != 3 || split(b, y, ".") != 3) return 1
    for (i = 1; i <= 3; i++) {
      if (x[i] !~ /^[0-9]+$/ || y[i] !~ /^[0-9]+$/) return 1
      if (x[i] + 0 != y[i] + 0) return x[i] + 0 < y[i] + 0
    }
    if (ap == "" || bp == "") return ap != "" && bp == ""
    return ap < bp
  }
  function older_than_all(v, list,   vs, n, i) {
    n = split(list, vs, " ")
    for (i = 1; i <= n; i++) if (!older(v, vs[i])) return 0
    return 1
  }
  # FILENAME rather than FNR == NR, which reads the findings of HEAD as
  # those of the baseline when the baseline has none.
  FILENAME == ARGV[1] {
    base[$1] = 1; seen[$1 SUBSEP $2] = 1; had[$1 SUBSEP $2 SUBSEP $3] = 1
    versions[$1 SUBSEP $2] = versions[$1 SUBSEP $2] " " $3
    next
  }
  { key[FNR] = $1; crate[FNR] = $2; version[FNR] = $3; text[FNR] = $4; n = FNR }
  END {
    for (i = 1; i <= n; i++) {
      kc = key[i] SUBSEP crate[i]
      if (!(key[i] in base)) print "new", text[i]
      else if (!(kc in seen))
        print "new", text[i] " -- already on the baseline, but it now reaches " crate[i] ", which it did not there"
      else if (!((kc SUBSEP version[i]) in had) && older_than_all(version[i], versions[kc]))
        print "new", text[i] " -- already on the baseline, but this copy is older than any the baseline had (" substr(versions[kc], 2) ")"
      else print "existing", text[i]
    }
  }
' "$workdir/base.tsv" "$workdir/head.tsv" > "$workdir/classified.tsv"
added=0 existing=0
while IFS=$'\t' read -r verdict description; do
  if [ "$verdict" = existing ]; then
    existing=$((existing + 1))
    echo "::warning title=Existing advisory::$description -- already on the baseline commit, so not failing this run"
  else
    added=$((added + 1))
    echo "::error title=New advisory::$description"
  fi
done < "$workdir/classified.tsv"

echo "$added new and $existing existing finding(s)$note"
[ "$added" -eq 0 ]
