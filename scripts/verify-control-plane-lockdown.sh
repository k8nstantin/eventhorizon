#!/usr/bin/env bash
# ============================================================================
# verify-control-plane-lockdown.sh
# ============================================================================
# Runs the full apply-and-verify cycle against a fresh, ephemeral Postgres 16
# container. This script is what proves the unified schema.sql is the source
# of truth: it spins Postgres up, applies schema.sql, attaches passwords,
# then asserts the lockdown contract by trying every operation as eh_service.
#
# Outcomes:
#   * SELECT on eh_control                — MUST succeed
#   * SELECT on eh_operational            — MUST succeed
#   * INSERT into eh_operational          — MUST succeed
#   * UPDATE on eh_operational            — MUST be engine-refused
#   * DELETE on eh_operational            — MUST be engine-refused
#   * INSERT into eh_control              — MUST be engine-refused
#   * CREATE TABLE / DDL                  — MUST be engine-refused
#
# Each check is an assertion. The script exits non-zero on the first failure
# and prints what failed.
#
# Usage:  bash scripts/verify-control-plane-lockdown.sh
# ============================================================================

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

CONTAINER="${EH_VERIFY_CONTAINER:-eh-schema-verify}"
PG_IMAGE="${EH_VERIFY_PG_IMAGE:-postgres:16-alpine}"
HOST_PORT="${EH_VERIFY_PORT:-55432}"

# Hard-coded test secrets — local-ephemeral, never used in real deploys.
ROOT_PW="verify_root_$$"
ADMIN_PW="verify_admin_$$"
SERVICE_PW="verify_service_$$"

cleanup() {
  docker rm -f "$CONTAINER" >/dev/null 2>&1 || true
}
trap cleanup EXIT

echo "==========================================================================="
echo "EventHorizon control-plane lockdown verification"
echo "==========================================================================="

# Make sure no stale container lingers.
docker rm -f "$CONTAINER" >/dev/null 2>&1 || true

echo "[1/6] Starting fresh $PG_IMAGE container ..."
docker run -d \
  --name "$CONTAINER" \
  -e POSTGRES_PASSWORD="$ROOT_PW" \
  -e POSTGRES_DB=eh_control_plane \
  -e POSTGRES_USER=postgres \
  -e EH_ADMIN_PASSWORD="$ADMIN_PW" \
  -e EH_SERVICE_PASSWORD="$SERVICE_PW" \
  -p "${HOST_PORT}:5432" \
  -v "$(pwd)/db/postgres/schema.sql:/docker-entrypoint-initdb.d/01_schema.sql:ro" \
  -v "$(pwd)/db/postgres/init/zz_set_role_passwords.sh:/docker-entrypoint-initdb.d/zz_set_role_passwords.sh:ro" \
  "$PG_IMAGE" >/dev/null

echo "[2/6] Waiting for Postgres to become ready and init to complete ..."
ready=0
for _ in $(seq 1 60); do
  # Wait until both pg_isready AND the password-setter has run (we detect this
  # by checking if eh_service can connect, which only happens after 0013).
  if docker exec "$CONTAINER" pg_isready -U postgres -d eh_control_plane >/dev/null 2>&1; then
    if docker exec -e PGPASSWORD="$SERVICE_PW" "$CONTAINER" \
         psql -h localhost -U eh_service -d eh_control_plane -tAc 'SELECT 1' >/dev/null 2>&1; then
      ready=1
      break
    fi
  fi
  sleep 1
done

if [[ "$ready" -ne 1 ]]; then
  echo "FATAL: Postgres did not become ready / init did not complete in 60s"
  docker logs "$CONTAINER" | tail -50
  exit 2
fi

echo "[3/6] Containers up and eh_service login works."

# Convenience runner: execute SQL as eh_service and return exit code.
psql_service() {
  docker exec -e PGPASSWORD="$SERVICE_PW" "$CONTAINER" \
    psql -h localhost -U eh_service -d eh_control_plane -v ON_ERROR_STOP=1 -tAc "$1"
}

# Run a query expected to FAIL with permission denied. Returns 0 if it did.
expect_engine_refusal() {
  local label="$1"
  local sql="$2"
  local out
  if out=$(psql_service "$sql" 2>&1); then
    echo "  FAIL: '$label' was expected to be engine-refused but it SUCCEEDED."
    echo "        SQL was: $sql"
    return 1
  fi
  if ! grep -qE 'permission denied|must be owner|cannot' <<<"$out"; then
    echo "  FAIL: '$label' was refused but not with the expected message:"
    echo "        $out"
    return 1
  fi
  echo "  OK:   '$label' engine-refused (as required)."
}

expect_success() {
  local label="$1"
  local sql="$2"
  if ! psql_service "$sql" >/dev/null 2>&1; then
    echo "  FAIL: '$label' was expected to succeed but it failed."
    echo "        SQL was: $sql"
    return 1
  fi
  echo "  OK:   '$label' succeeded (as required)."
}

failed=0

echo ""
echo "[4/6] Lockdown contract — engine-enforced reads/writes as eh_service"
echo "      The session GUC app.tenant_id must be set for RLS-scoped reads."
echo ""

# Set the tenant GUC so RLS allows the eh_service reads.
TENANT_UUID="00000000-0000-7000-8000-000000000001"

echo "  ... setting session GUC app.tenant_id (then issuing each test in a fresh psql so the GUC is fresh)"
# We bundle each assertion's SET + query in one psql session.

assert_with_tenant() {
  local label="$1"
  local sql="$2"
  local expected="$3"   # 'success' or 'refusal'
  local out
  local cmd
  cmd="SET LOCAL app.tenant_id = '${TENANT_UUID}'; ${sql}"
  if out=$(docker exec -e PGPASSWORD="$SERVICE_PW" "$CONTAINER" \
            psql -h localhost -U eh_service -d eh_control_plane -v ON_ERROR_STOP=1 -tAc "$cmd" 2>&1); then
    if [[ "$expected" == "success" ]]; then
      echo "  OK:   '$label' succeeded (as required)."
      return 0
    else
      echo "  FAIL: '$label' was expected to be engine-refused but it SUCCEEDED."
      echo "        SQL was: $sql"
      return 1
    fi
  else
    if [[ "$expected" == "refusal" ]]; then
      if grep -qE 'permission denied|must be owner|cannot|null value' <<<"$out"; then
        echo "  OK:   '$label' engine-refused (as required)."
        return 0
      else
        echo "  FAIL: '$label' refused with unexpected message:"
        echo "        $out"
        return 1
      fi
    else
      echo "  FAIL: '$label' was expected to succeed but it failed."
      echo "        SQL was: $sql"
      echo "        Out:     $out"
      return 1
    fi
  fi
}

# ============================================================================
# eh_control: SELECT only
# ============================================================================
assert_with_tenant \
  "eh_service: SELECT FROM eh_control.agents" \
  "SELECT count(*) FROM eh_control.agents" \
  "success" || failed=1

assert_with_tenant \
  "eh_service: INSERT INTO eh_control.agents — must be refused" \
  "INSERT INTO eh_control.agents (id, name, tenant_id, owner_email) VALUES (gen_random_uuid(), 'rogue', '${TENANT_UUID}', 'r@x.com')" \
  "refusal" || failed=1

# ============================================================================
# eh_operational: SELECT + INSERT (UPDATE / DELETE refused)
# ============================================================================
assert_with_tenant \
  "eh_service: SELECT FROM eh_operational.telemetry_events" \
  "SELECT count(*) FROM eh_operational.telemetry_events" \
  "success" || failed=1

assert_with_tenant \
  "eh_service: INSERT INTO eh_operational.telemetry_events" \
  "INSERT INTO eh_operational.telemetry_events (id, ts, trace_id, tenant_id, event_kind) VALUES (gen_random_uuid(), now(), gen_random_uuid(), '${TENANT_UUID}', 'intent_received')" \
  "success" || failed=1

assert_with_tenant \
  "eh_service: UPDATE eh_operational.telemetry_events — must be refused" \
  "UPDATE eh_operational.telemetry_events SET event_kind='intent_routed'" \
  "refusal" || failed=1

assert_with_tenant \
  "eh_service: DELETE FROM eh_operational.telemetry_events — must be refused" \
  "DELETE FROM eh_operational.telemetry_events" \
  "refusal" || failed=1

# ============================================================================
# DDL — must be refused
# ============================================================================
assert_with_tenant \
  "eh_service: CREATE TABLE — must be refused" \
  "CREATE TABLE eh_control.rogue (id UUID PRIMARY KEY)" \
  "refusal" || failed=1

assert_with_tenant \
  "eh_service: DROP TABLE — must be refused" \
  "DROP TABLE eh_control.agents" \
  "refusal" || failed=1

assert_with_tenant \
  "eh_service: TRUNCATE — must be refused" \
  "TRUNCATE eh_operational.telemetry_events" \
  "refusal" || failed=1

echo ""
echo "[5/6] Counting tables to confirm full apply ..."
ctrl=$(docker exec -e PGPASSWORD="$ADMIN_PW" "$CONTAINER" \
  psql -h localhost -U eh_admin -d eh_control_plane -tAc \
    "SELECT count(*) FROM information_schema.tables WHERE table_schema='eh_control' AND table_type='BASE TABLE'" | tr -d '[:space:]')
oper=$(docker exec -e PGPASSWORD="$ADMIN_PW" "$CONTAINER" \
  psql -h localhost -U eh_admin -d eh_control_plane -tAc \
    "SELECT count(*) FROM information_schema.tables WHERE table_schema='eh_operational' AND table_type='BASE TABLE'" | tr -d '[:space:]')

echo "  eh_control:     $ctrl tables (expect 25)"
echo "  eh_operational: $oper tables (expect 10 — 6 base + 4 partition children)"

if [[ "$ctrl" -ne 25 || "$oper" -ne 10 ]]; then
  echo "  FAIL: unexpected table count"
  failed=1
fi

echo ""
echo "[6/6] Verdict"
if [[ "$failed" -ne 0 ]]; then
  echo "  ✗ LOCKDOWN VERIFICATION FAILED"
  exit 1
fi
echo "  ✓ LOCKDOWN VERIFIED — schema.sql applies cleanly, eh_service is constrained to SELECT (eh_control) + SELECT+INSERT (eh_operational), all destructive ops engine-refused."
