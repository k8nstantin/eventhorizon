#!/usr/bin/env bash
# ============================================================================
# check-no-admin-creds.sh
# ============================================================================
# CI guardrail. Application code (anything under crates/) MUST operate within
# the eh_service scope. Per zero-trust §10 / §13 and SERVICE_ACCOUNT.md:
#
#   * NEVER hardcode references to OUR project's admin / root credentials
#     (eh_admin, EH_ADMIN_PASSWORD, etc.). The application binary's identity
#     is eh_service, sourced from operator-provided env-injected secrets.
#   * NEVER issue SQL mutations other than INSERT. SELECT and INSERT are
#     the entire SQL vocabulary the application is allowed.
#
# SCOPE CLARIFICATION:
#   This guardrail is about OUR own admin secrets (the control-plane
#   eh_admin role, the FVP-shipped MySQL admin/root, etc.). It is NOT
#   about restricting operators' freedom to configure their connector
#   backends with whatever credentials they choose. An operator who
#   sets `username: root` in their tenant connector YAML is exercising
#   legitimate choice — the connector code reads that config and connects.
#   The values the connector reads are operator-supplied at runtime via
#   ${ENV:NAME} refs, not Rust string literals.
#
# Exempt locations:
#   * db/                       migrations, init scripts, password setters
#   * scripts/                  operator tooling
#   * docker-compose.yml + .env.example
#   * *.md                      documentation may mention forbidden tokens
#   * .github/                  CI itself
#
# Forbidden in crates/**/*.rs:
#
#   OUR ADMIN IDENTIFIERS / CREDENTIAL ENV-VAR NAMES
#     eh_admin                   Postgres admin role (our schema)
#     EH_ADMIN_PASSWORD          our admin password env-var name
#     POSTGRES_ROOT_PASSWORD     PG superuser password env-var name
#     MYSQL_ROOT_PASSWORD        FVP-shipped MySQL superuser env-var name
#     MYSQL_EH_ADMIN_PASSWORD    FVP-shipped MySQL admin env-var name
#
#   SQL MUTATION KEYWORDS (in likely-SQL string literals)
#     "UPDATE                    SQL UPDATE statement
#     "DELETE FROM               SQL DELETE statement
#     "DROP                      SQL DROP (table/index/role/…)
#     "ALTER                     SQL ALTER
#     "TRUNCATE                  SQL TRUNCATE
#     ON CONFLICT                Postgres upsert clause (UPDATE-shaped path)
#     ON DUPLICATE KEY UPDATE    MySQL upsert clause
#     UPSERT                     CockroachDB / Spanner upsert
#
# Run locally:  bash scripts/check-no-admin-creds.sh
# ============================================================================

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

# --- (A) Our own admin identifiers / env-var names (hardcoded refs forbidden) -
admin_patterns=(
    'eh_admin'
    'EH_ADMIN_PASSWORD'
    'POSTGRES_ROOT_PASSWORD'
    'MYSQL_ROOT_PASSWORD'
    'MYSQL_EH_ADMIN_PASSWORD'
)

# --- (B) SQL mutation keywords inside likely string literals ---------------
mutation_patterns=(
    '"UPDATE '
    '"DELETE FROM'
    '"DROP '
    '"ALTER '
    '"TRUNCATE'
    "'UPDATE "
    "'DELETE FROM"
    "'DROP "
    "'ALTER "
    "'TRUNCATE"
    'r"UPDATE '
    'r"DELETE FROM'
    'r"DROP '
    'r"ALTER '
    'r#"UPDATE '
    'r#"DELETE FROM'
    'r#"DROP '
    'r#"ALTER '
    'ON CONFLICT'
    'ON DUPLICATE KEY UPDATE'
    '"UPSERT'
    "'UPSERT"
)

if [[ ! -d crates ]]; then
    echo "OK: no crates/ directory yet (pre-Phase-1)."
    exit 0
fi

failed=0

scan() {
    local label="$1"
    shift
    local patterns=("$@")
    for pattern in "${patterns[@]}"; do
        matches=$(grep -rn --include='*.rs' -F "$pattern" crates/ 2>/dev/null || true)
        if [[ -n "$matches" ]]; then
            echo "FAIL ($label): forbidden token '${pattern}' in Rust source:"
            printf '%s\n' "$matches" | sed 's/^/  /'
            echo ""
            failed=1
        fi
    done
}

scan "admin-creds"  "${admin_patterns[@]}"
scan "sql-mutation" "${mutation_patterns[@]}"

if [[ "$failed" -eq 0 ]]; then
    echo "OK: application code (crates/) holds no hardcoded admin/root credential"
    echo "    references and uses only SELECT/INSERT-shaped SQL (eh_service scope"
    echo "    per zero-trust §13)."
    exit 0
fi

cat <<'EOF'

Per zero-trust §13 + SERVICE_ACCOUNT.md, application code:
  * authenticates ONLY as eh_service to our own resources, and
  * issues ONLY SELECT and INSERT SQL.

Operator-configured connector credentials (read from YAML config via
${ENV:NAME} refs at runtime) are NOT restricted by this check — that is
the operator's choice. This check ONLY forbids hardcoded references to
our own admin identifiers and forbidden SQL mutations in Rust source.

If the code needs an operation eh_service cannot perform, do NOT bypass —
escalate per SERVICE_ACCOUNT.md "Escalation path" (open issue, discuss
with operator, change the schema or grants properly, then app code follows).

Engine refusals are signals, not obstacles.
EOF

exit 1
