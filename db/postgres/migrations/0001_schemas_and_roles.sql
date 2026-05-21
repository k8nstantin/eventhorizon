-- ============================================================================
-- 0001 — Schemas and roles
-- ============================================================================
-- Creates the two logical schemas (eh_control + eh_operational) and the two
-- application roles (eh_admin, eh_service). Default privileges are set so
-- every future table inherits the right grants without per-table GRANT
-- statements.
--
-- Reference: architecture §5.1–§5.10, SCHEMA.md §2, §5.
-- ============================================================================

-- 1. Schemas
-- ----------------------------------------------------------------------------

CREATE SCHEMA IF NOT EXISTS eh_control;
CREATE SCHEMA IF NOT EXISTS eh_operational;

COMMENT ON SCHEMA eh_control
  IS 'Config: agents, sources, entities, bindings, rules, policies. Small, slow-changing, operator-managed.';

COMMENT ON SCHEMA eh_operational
  IS 'Events: audit log, telemetry, health, drift, cost, proposals. Large, append-only, rebuildable from Kafka mirror.';

-- 2. Roles
-- ----------------------------------------------------------------------------
-- Role passwords are NOT set here. Operator sets them via secrets manager
-- and connects with the appropriate credentials. CREATE ROLE here only
-- declares the role identities.
-- ----------------------------------------------------------------------------

DO $$
BEGIN
  IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'eh_admin') THEN
    CREATE ROLE eh_admin WITH LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE INHERIT;
    COMMENT ON ROLE eh_admin
      IS 'Operator-only role. Holds DDL + GRANT + REVOKE + migration authority. Application NEVER authenticates as this role (zero-trust §13).';
  END IF;

  IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'eh_service') THEN
    CREATE ROLE eh_service WITH LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE INHERIT;
    COMMENT ON ROLE eh_service
      IS 'Application role. SELECT on eh_control; SELECT + INSERT on eh_operational. No UPDATE, no DELETE, no DDL — engine-refused.';
  END IF;
END
$$;

-- 3. Schema usage grants
-- ----------------------------------------------------------------------------

GRANT USAGE ON SCHEMA eh_control     TO eh_service;
GRANT USAGE ON SCHEMA eh_operational TO eh_service;

GRANT ALL    ON SCHEMA eh_control     TO eh_admin;
GRANT ALL    ON SCHEMA eh_operational TO eh_admin;

-- 4. Default privileges for FUTURE tables
-- ----------------------------------------------------------------------------
-- Every table created hereafter inherits these grants automatically.
-- This is what enforces "the app cannot UPDATE or DELETE" at the engine
-- level — eh_service simply has no UPDATE/DELETE privilege ever granted.
-- ----------------------------------------------------------------------------

-- eh_control: app gets SELECT only on every future table.
ALTER DEFAULT PRIVILEGES IN SCHEMA eh_control
  GRANT SELECT ON TABLES TO eh_service;

-- eh_operational: app gets SELECT + INSERT only.
ALTER DEFAULT PRIVILEGES IN SCHEMA eh_operational
  GRANT SELECT, INSERT ON TABLES TO eh_service;

-- Sequences (in case SCD2 chain-key sequences are added later — currently
-- everything uses UUIDv7 app-side, so no sequences are expected, but we
-- grant USAGE prophylactically for future flexibility).
ALTER DEFAULT PRIVILEGES IN SCHEMA eh_control
  GRANT USAGE ON SEQUENCES TO eh_service;
ALTER DEFAULT PRIVILEGES IN SCHEMA eh_operational
  GRANT USAGE ON SEQUENCES TO eh_service;

-- Admin role gets everything on every future object.
ALTER DEFAULT PRIVILEGES IN SCHEMA eh_control
  GRANT ALL ON TABLES TO eh_admin;
ALTER DEFAULT PRIVILEGES IN SCHEMA eh_operational
  GRANT ALL ON TABLES TO eh_admin;
ALTER DEFAULT PRIVILEGES IN SCHEMA eh_control
  GRANT ALL ON SEQUENCES TO eh_admin;
ALTER DEFAULT PRIVILEGES IN SCHEMA eh_operational
  GRANT ALL ON SEQUENCES TO eh_admin;

-- 5. Required extensions
-- ----------------------------------------------------------------------------

-- pgcrypto is used opportunistically (gen_random_uuid for emergencies);
-- UUIDv7 PKs are app-generated. This extension is broadly available on
-- managed Postgres providers.
CREATE EXTENSION IF NOT EXISTS pgcrypto;

-- 6. Session-variable contract for identity passthrough (§5.10, §7)
-- ----------------------------------------------------------------------------
-- The application sets `app.agent_id` and `app.tenant_id` per request via
-- `SET LOCAL` inside its transaction. RLS policies (migration 0011) read
-- these via current_setting() to filter rows. No object-level config needed
-- here — Postgres GUCs of the form `<namespace>.<name>` are accepted by
-- default; we just document the contract.
-- ----------------------------------------------------------------------------
