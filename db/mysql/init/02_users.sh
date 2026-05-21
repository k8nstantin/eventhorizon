#!/bin/bash
# ============================================================================
# 02_users.sh — Create eh_admin and eh_service users with grants
# ============================================================================
# Runs after 01_fvp_schema.sql via the MySQL Docker entrypoint init mechanism.
# Reads passwords from env vars injected by docker-compose (which in turn
# reads them from the operator's secrets manager — never embedded).
#
# Grant set (unchanged from the v1 schema lock):
#   * eh_admin   — ALL PRIVILEGES on eh_demo.*           (operator only)
#   * eh_service — SELECT on eh_demo.customers           (Phase 1 FVP)
#                  Explicitly NO INSERT, UPDATE, DELETE, DDL.
#
# Reference: SCHEMA.md §5.3, zero-trust §10 / §13.
# ============================================================================

set -euo pipefail

: "${MYSQL_ROOT_PASSWORD:?MYSQL_ROOT_PASSWORD must be set by the entrypoint env}"
: "${MYSQL_EH_ADMIN_PASSWORD:?MYSQL_EH_ADMIN_PASSWORD must be set (operator secret)}"
: "${MYSQL_EH_SERVICE_PASSWORD:?MYSQL_EH_SERVICE_PASSWORD must be set (operator secret)}"

# Reject passwords that contain quote chars so the SQL below cannot be
# closed-out by hostile input. Operator should ensure passwords are
# alphanumeric + symbol subset that does not require shell escaping.
for var in MYSQL_EH_ADMIN_PASSWORD MYSQL_EH_SERVICE_PASSWORD; do
    val="${!var}"
    if [[ "$val" == *"'"* || "$val" == *'"'* || "$val" == *'\\'* ]]; then
        echo "ERROR: $var contains a quote or backslash. Choose a password without those characters." >&2
        exit 1
    fi
done

# Use MYSQL_PWD env var so the password never appears on the command line
# (would be visible via `ps`).
export MYSQL_PWD="$MYSQL_ROOT_PASSWORD"

mysql -uroot --protocol=socket <<EOF
CREATE USER IF NOT EXISTS 'eh_admin'@'%'   IDENTIFIED BY '${MYSQL_EH_ADMIN_PASSWORD}';
GRANT ALL PRIVILEGES ON eh_demo.* TO 'eh_admin'@'%';

CREATE USER IF NOT EXISTS 'eh_service'@'%' IDENTIFIED BY '${MYSQL_EH_SERVICE_PASSWORD}';
GRANT SELECT ON eh_demo.customers TO 'eh_service'@'%';

FLUSH PRIVILEGES;
EOF

unset MYSQL_PWD

echo "02_users.sh: created eh_admin and eh_service users with locked grants."
