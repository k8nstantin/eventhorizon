#!/usr/bin/env bash
# ============================================================================
# check-schema-sync.sh
# ============================================================================
# CI guardrail: assert that the set of tables documented in SCHEMA.md matches
# the set of tables declared in db/postgres/migrations/*.sql and
# db/mysql/init/*.sql.
#
# If the doc and the DDL drift, this script fails. The schema-first workflow
# (zero-trust §11) requires both to evolve together.
# ============================================================================

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

# ----------------------------------------------------------------------------
# 1. Tables declared in DDL
# ----------------------------------------------------------------------------
# Match only top-of-line `CREATE TABLE <schema>.<name>` (Postgres) or
# `CREATE TABLE [IF NOT EXISTS] <name>` (MySQL).  awk reliably extracts the
# fully-qualified name regardless of trailing `(` or newline.
# ----------------------------------------------------------------------------

declared_pg=$(
  awk '
    /^CREATE TABLE / {
      line = $0;
      sub(/^CREATE TABLE /, "", line);
      sub(/^IF NOT EXISTS /, "", line);
      # Take the first whitespace- or paren-delimited token (the table name).
      n = split(line, parts, /[[:space:](]/);
      name = parts[1];
      if (name ~ /^(eh_control|eh_operational)\./) print name;
    }
  ' db/postgres/migrations/*.sql | sort -u
)

declared_mysql=$(
  awk '
    /^CREATE TABLE / {
      line = $0;
      sub(/^CREATE TABLE /, "", line);
      sub(/^IF NOT EXISTS /, "", line);
      n = split(line, parts, /[[:space:](]/);
      name = parts[1];
      if (name !~ /\./) print "eh_demo." name;
      else print name;
    }
  ' db/mysql/init/*.sql | sort -u
)

declared=$(printf '%s\n%s\n' "$declared_pg" "$declared_mysql" | sed '/^$/d' | sort -u)

# ----------------------------------------------------------------------------
# 2. Tables documented in SCHEMA.md
# ----------------------------------------------------------------------------
# Pull fully-qualified table names that appear anywhere in SCHEMA.md.
# We capture: eh_control.<name>, eh_operational.<name>, eh_demo.<name>.
# ----------------------------------------------------------------------------

documented=$(
  grep -hoE '\b(eh_control|eh_operational|eh_demo)\.[a-z_][a-z0-9_]*\b' SCHEMA.md \
    | grep -Ev '_$' \
    | sort -u
)

# ----------------------------------------------------------------------------
# 3. Strip partition children — they're DDL artifacts of their parent table
#    and don't need separate SCHEMA.md entries.
# ----------------------------------------------------------------------------

declared=$(
  printf '%s\n' "$declared" \
    | grep -Ev '_(default|[0-9]{4}_[0-9]{2})$' \
    || true
)

# ----------------------------------------------------------------------------
# 4. Compute diff
# ----------------------------------------------------------------------------

only_in_ddl=$(comm -23 <(printf '%s\n' "$declared") <(printf '%s\n' "$documented") || true)
only_in_doc=$(comm -13 <(printf '%s\n' "$declared") <(printf '%s\n' "$documented") || true)

failed=0

if [[ -n "$only_in_ddl" ]]; then
  echo "ERROR: tables declared in DDL but NOT documented in SCHEMA.md:"
  printf '  - %s\n' $only_in_ddl
  failed=1
fi

if [[ -n "$only_in_doc" ]]; then
  echo "ERROR: tables documented in SCHEMA.md but NOT declared in DDL:"
  printf '  - %s\n' $only_in_doc
  failed=1
fi

if [[ "$failed" -eq 0 ]]; then
  count=$(printf '%s\n' "$declared" | grep -c . || true)
  echo "OK: SCHEMA.md and DDL agree on $count tables."
  exit 0
fi

echo ""
echo "Per zero-trust §11 (Schema-First, Code-After), SCHEMA.md and the DDL"
echo "must evolve together. Update both in the same PR; the operator approves"
echo "schema changes per-table."
exit 1
