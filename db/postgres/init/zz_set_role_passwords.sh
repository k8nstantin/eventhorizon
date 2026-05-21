#!/bin/bash
# ============================================================================
# 0013_set_role_passwords.sh — Set eh_admin and eh_service role passwords
# ============================================================================
# Runs as the LAST init step in /docker-entrypoint-initdb.d (alphabetical
# order: 0013 sorts after 0012, before any 'z*' files). Reads passwords from
# env vars injected by docker-compose (which sources them from the operator's
# secrets manager / .env file — never embedded).
#
# Role definitions (CREATE ROLE) were created with LOGIN but no password in
# migration 0001_schemas_and_roles.sql. This script attaches passwords
# without ever placing them in version-controlled SQL.
#
# Idempotency: ALTER ROLE … PASSWORD is idempotent; running this script
# again on a re-initialized cluster sets fresh passwords.
#
# Reference: SCHEMA.md §5.1, zero-trust §13 (Database Credentials).
# ============================================================================

set -euo pipefail

: "${EH_ADMIN_PASSWORD:?EH_ADMIN_PASSWORD must be set by the compose env (operator secret)}"
: "${EH_SERVICE_PASSWORD:?EH_SERVICE_PASSWORD must be set by the compose env (operator secret)}"

# Reject passwords with characters that would let the SQL below be closed-out
# by hostile input. Operators should pick alphanumeric + symbol passwords that
# do not contain quotes or backslashes.
for var in EH_ADMIN_PASSWORD EH_SERVICE_PASSWORD; do
    val="${!var}"
    if [[ "$val" == *"'"* || "$val" == *'\\'* ]]; then
        echo "ERROR: $var contains a single-quote or backslash; pick a password without those." >&2
        exit 1
    fi
done

# Use PGPASSWORD so the superuser password never appears on the command line.
# POSTGRES_PASSWORD is set by the docker-entrypoint as the superuser's password.
export PGPASSWORD="$POSTGRES_PASSWORD"

psql --username "$POSTGRES_USER" --dbname "$POSTGRES_DB" --set ON_ERROR_STOP=1 <<EOF
ALTER ROLE eh_admin   WITH PASSWORD '${EH_ADMIN_PASSWORD}';
ALTER ROLE eh_service WITH PASSWORD '${EH_SERVICE_PASSWORD}';
EOF

unset PGPASSWORD

echo "0013_set_role_passwords.sh: eh_admin and eh_service passwords attached."
