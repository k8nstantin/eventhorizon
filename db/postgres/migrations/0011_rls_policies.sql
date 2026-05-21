-- ============================================================================
-- 0011 — Row-level security (RLS) policies
-- ============================================================================
-- Defense-in-depth on top of the schema-level grants. The application sets
-- app.tenant_id (and app.agent_id) via SET LOCAL inside its request
-- transaction; these policies filter rows accordingly.
--
-- eh_admin bypasses RLS (it's the operator escape hatch).
-- eh_service is subject to all RLS policies.
--
-- Reference: architecture §5.8, §5.10, §8.
-- ============================================================================

-- Helper: a stable function returning the current tenant from the GUC.
-- Marked STABLE so it can be used in policy expressions efficiently.
-- ----------------------------------------------------------------------------

CREATE OR REPLACE FUNCTION eh_control.current_tenant_id() RETURNS UUID AS $$
  SELECT NULLIF(current_setting('app.tenant_id', true), '')::UUID;
$$ LANGUAGE SQL STABLE SECURITY INVOKER;

COMMENT ON FUNCTION eh_control.current_tenant_id()
  IS 'Returns the tenant_id from the per-request session GUC app.tenant_id. NULL if unset (request rejected by upstream guard before reaching SQL).';

-- ============================================================================
-- 1. eh_control — tenant-scoped tables
-- ============================================================================

-- 1.1 tenants — eh_service can read its own row only.
ALTER TABLE eh_control.tenants ENABLE ROW LEVEL SECURITY;
ALTER TABLE eh_control.tenants FORCE ROW LEVEL SECURITY;

CREATE POLICY tenants_select_own ON eh_control.tenants
  FOR SELECT TO eh_service
  USING (id = eh_control.current_tenant_id());

-- 1.2 agents — scoped by tenant_id.
ALTER TABLE eh_control.agents ENABLE ROW LEVEL SECURITY;
ALTER TABLE eh_control.agents FORCE ROW LEVEL SECURITY;

CREATE POLICY agents_select_tenant ON eh_control.agents
  FOR SELECT TO eh_service
  USING (tenant_id = eh_control.current_tenant_id());

-- 1.3 agent_secrets — scoped via agent ownership.
ALTER TABLE eh_control.agent_secrets ENABLE ROW LEVEL SECURITY;
ALTER TABLE eh_control.agent_secrets FORCE ROW LEVEL SECURITY;

CREATE POLICY agent_secrets_select_tenant ON eh_control.agent_secrets
  FOR SELECT TO eh_service
  USING (
    EXISTS (
      SELECT 1 FROM eh_control.agents a
      WHERE a.id = agent_secrets.agent_id
        AND a.tenant_id = eh_control.current_tenant_id()
    )
  );

-- 1.4 policies — scoped by tenant_id.
ALTER TABLE eh_control.policies ENABLE ROW LEVEL SECURITY;
ALTER TABLE eh_control.policies FORCE ROW LEVEL SECURITY;

CREATE POLICY policies_select_tenant ON eh_control.policies
  FOR SELECT TO eh_service
  USING (tenant_id = eh_control.current_tenant_id());

-- 1.5 capabilities — scoped via agent ownership.
ALTER TABLE eh_control.capabilities ENABLE ROW LEVEL SECURITY;
ALTER TABLE eh_control.capabilities FORCE ROW LEVEL SECURITY;

CREATE POLICY capabilities_select_tenant ON eh_control.capabilities
  FOR SELECT TO eh_service
  USING (
    EXISTS (
      SELECT 1 FROM eh_control.agents a
      WHERE a.id = capabilities.agent_id
        AND a.tenant_id = eh_control.current_tenant_id()
    )
  );

-- 1.6 sources — scoped by tenant_id.
ALTER TABLE eh_control.sources ENABLE ROW LEVEL SECURITY;
ALTER TABLE eh_control.sources FORCE ROW LEVEL SECURITY;

CREATE POLICY sources_select_tenant ON eh_control.sources
  FOR SELECT TO eh_service
  USING (tenant_id = eh_control.current_tenant_id());

-- 1.7 source_credentials — scoped via source.tenant.
ALTER TABLE eh_control.source_credentials ENABLE ROW LEVEL SECURITY;
ALTER TABLE eh_control.source_credentials FORCE ROW LEVEL SECURITY;

CREATE POLICY source_credentials_select_tenant ON eh_control.source_credentials
  FOR SELECT TO eh_service
  USING (
    EXISTS (
      SELECT 1 FROM eh_control.sources s
      WHERE s.id = source_credentials.source_id
        AND s.tenant_id = eh_control.current_tenant_id()
    )
  );

-- 1.8 source_<kind> tables — scoped via source.tenant.
-- One policy per per-kind table. Pattern is identical; we write them out so
-- behaviour is auditable per-table.
-- ----------------------------------------------------------------------------

DO $$
DECLARE
  t TEXT;
BEGIN
  FOR t IN SELECT unnest(ARRAY[
    'source_mysql',
    'source_postgres',
    'source_iceberg',
    'source_duckdb',
    'source_rag',
    'source_model',
    'source_file',
    'source_snowflake',
    'source_mssql'
  ]) LOOP
    EXECUTE format('ALTER TABLE eh_control.%I ENABLE ROW LEVEL SECURITY;', t);
    EXECUTE format('ALTER TABLE eh_control.%I FORCE ROW LEVEL SECURITY;', t);
    EXECUTE format(
      'CREATE POLICY %I_select_tenant ON eh_control.%I FOR SELECT TO eh_service USING (EXISTS (SELECT 1 FROM eh_control.sources s WHERE s.id = %I.source_id AND s.tenant_id = eh_control.current_tenant_id()));',
      t, t, t
    );
  END LOOP;
END
$$;

-- 1.9 entities — scoped by tenant_id.
ALTER TABLE eh_control.entities ENABLE ROW LEVEL SECURITY;
ALTER TABLE eh_control.entities FORCE ROW LEVEL SECURITY;

CREATE POLICY entities_select_tenant ON eh_control.entities
  FOR SELECT TO eh_service
  USING (tenant_id = eh_control.current_tenant_id());

-- 1.10 entity_fields — scoped via entity.tenant.
ALTER TABLE eh_control.entity_fields ENABLE ROW LEVEL SECURITY;
ALTER TABLE eh_control.entity_fields FORCE ROW LEVEL SECURITY;

CREATE POLICY entity_fields_select_tenant ON eh_control.entity_fields
  FOR SELECT TO eh_service
  USING (
    EXISTS (
      SELECT 1 FROM eh_control.entities e
      WHERE e.id = entity_fields.entity_id
        AND e.tenant_id = eh_control.current_tenant_id()
    )
  );

-- 1.11 entity_bindings — scoped via entity.tenant.
ALTER TABLE eh_control.entity_bindings ENABLE ROW LEVEL SECURITY;
ALTER TABLE eh_control.entity_bindings FORCE ROW LEVEL SECURITY;

CREATE POLICY entity_bindings_select_tenant ON eh_control.entity_bindings
  FOR SELECT TO eh_service
  USING (
    EXISTS (
      SELECT 1 FROM eh_control.entities e
      WHERE e.id = entity_bindings.entity_id
        AND e.tenant_id = eh_control.current_tenant_id()
    )
  );

-- 1.12 entity_field_bindings — scoped via binding → entity → tenant.
ALTER TABLE eh_control.entity_field_bindings ENABLE ROW LEVEL SECURITY;
ALTER TABLE eh_control.entity_field_bindings FORCE ROW LEVEL SECURITY;

CREATE POLICY entity_field_bindings_select_tenant ON eh_control.entity_field_bindings
  FOR SELECT TO eh_service
  USING (
    EXISTS (
      SELECT 1
      FROM eh_control.entity_bindings b
      JOIN eh_control.entities e ON e.id = b.entity_id
      WHERE b.id = entity_field_bindings.binding_id
        AND e.tenant_id = eh_control.current_tenant_id()
    )
  );

-- 1.13 routing_rules — scoped via entity.tenant.
ALTER TABLE eh_control.routing_rules ENABLE ROW LEVEL SECURITY;
ALTER TABLE eh_control.routing_rules FORCE ROW LEVEL SECURITY;

CREATE POLICY routing_rules_select_tenant ON eh_control.routing_rules
  FOR SELECT TO eh_service
  USING (
    EXISTS (
      SELECT 1 FROM eh_control.entities e
      WHERE e.id = routing_rules.entity_id
        AND e.tenant_id = eh_control.current_tenant_id()
    )
  );

-- 1.14 predicate_nodes — scoped via rule → entity → tenant.
ALTER TABLE eh_control.predicate_nodes ENABLE ROW LEVEL SECURITY;
ALTER TABLE eh_control.predicate_nodes FORCE ROW LEVEL SECURITY;

CREATE POLICY predicate_nodes_select_tenant ON eh_control.predicate_nodes
  FOR SELECT TO eh_service
  USING (
    EXISTS (
      SELECT 1
      FROM eh_control.routing_rules r
      JOIN eh_control.entities e ON e.id = r.entity_id
      WHERE r.id = predicate_nodes.rule_id
        AND e.tenant_id = eh_control.current_tenant_id()
    )
  );

-- 1.15 entity_relationships — scoped opportunistically by from-side tenant
-- (relationships have no native tenant column; we accept reads where either
-- side's entity table indicates the current tenant).
-- ----------------------------------------------------------------------------

ALTER TABLE eh_control.entity_relationships ENABLE ROW LEVEL SECURITY;
ALTER TABLE eh_control.entity_relationships FORCE ROW LEVEL SECURITY;

-- Bare-minimum policy for V1: allow only relationships whose from_kind=='entity'
-- and from_id resolves to a same-tenant entity. Cross-kind queries from
-- eh_service are intentionally narrow in V1; expand in later phases.
CREATE POLICY entity_relationships_select_tenant ON eh_control.entity_relationships
  FOR SELECT TO eh_service
  USING (
    (from_kind = 'entity' AND EXISTS (
      SELECT 1 FROM eh_control.entities e
      WHERE e.id = entity_relationships.from_id
        AND e.tenant_id = eh_control.current_tenant_id()
    ))
    OR
    (to_kind = 'entity' AND EXISTS (
      SELECT 1 FROM eh_control.entities e
      WHERE e.id = entity_relationships.to_id
        AND e.tenant_id = eh_control.current_tenant_id()
    ))
  );

-- ============================================================================
-- 2. eh_operational — tenant-scoped tables (INSERT + SELECT)
-- ============================================================================
-- For inserts, the WITH CHECK clause enforces that the new row's tenant_id
-- matches the session GUC. For selects, the USING clause filters.
-- ============================================================================

-- 2.1 audit_log — partitioned parent; RLS attaches to the parent and
-- inherits to all current and future partitions.
ALTER TABLE eh_operational.audit_log ENABLE ROW LEVEL SECURITY;
ALTER TABLE eh_operational.audit_log FORCE ROW LEVEL SECURITY;

CREATE POLICY audit_log_tenant_select ON eh_operational.audit_log
  FOR SELECT TO eh_service
  USING (tenant_id = eh_control.current_tenant_id());

CREATE POLICY audit_log_tenant_insert ON eh_operational.audit_log
  FOR INSERT TO eh_service
  WITH CHECK (tenant_id = eh_control.current_tenant_id());

-- 2.2 telemetry_events — partitioned parent.
ALTER TABLE eh_operational.telemetry_events ENABLE ROW LEVEL SECURITY;
ALTER TABLE eh_operational.telemetry_events FORCE ROW LEVEL SECURITY;

CREATE POLICY telemetry_events_tenant_select ON eh_operational.telemetry_events
  FOR SELECT TO eh_service
  USING (tenant_id = eh_control.current_tenant_id());

CREATE POLICY telemetry_events_tenant_insert ON eh_operational.telemetry_events
  FOR INSERT TO eh_service
  WITH CHECK (tenant_id = eh_control.current_tenant_id());

-- 2.3 source_health — scoped via source.tenant (no native tenant column).
ALTER TABLE eh_operational.source_health ENABLE ROW LEVEL SECURITY;
ALTER TABLE eh_operational.source_health FORCE ROW LEVEL SECURITY;

CREATE POLICY source_health_tenant_select ON eh_operational.source_health
  FOR SELECT TO eh_service
  USING (
    EXISTS (
      SELECT 1 FROM eh_control.sources s
      WHERE s.id = source_health.source_id
        AND s.tenant_id = eh_control.current_tenant_id()
    )
  );

CREATE POLICY source_health_tenant_insert ON eh_operational.source_health
  FOR INSERT TO eh_service
  WITH CHECK (
    EXISTS (
      SELECT 1 FROM eh_control.sources s
      WHERE s.id = source_health.source_id
        AND s.tenant_id = eh_control.current_tenant_id()
    )
  );

-- 2.4 schema_snapshots — same pattern as source_health.
ALTER TABLE eh_operational.schema_snapshots ENABLE ROW LEVEL SECURITY;
ALTER TABLE eh_operational.schema_snapshots FORCE ROW LEVEL SECURITY;

CREATE POLICY schema_snapshots_tenant_select ON eh_operational.schema_snapshots
  FOR SELECT TO eh_service
  USING (
    EXISTS (
      SELECT 1 FROM eh_control.sources s
      WHERE s.id = schema_snapshots.source_id
        AND s.tenant_id = eh_control.current_tenant_id()
    )
  );

CREATE POLICY schema_snapshots_tenant_insert ON eh_operational.schema_snapshots
  FOR INSERT TO eh_service
  WITH CHECK (
    EXISTS (
      SELECT 1 FROM eh_control.sources s
      WHERE s.id = schema_snapshots.source_id
        AND s.tenant_id = eh_control.current_tenant_id()
    )
  );

-- 2.5 cost_records — has native tenant_id column.
ALTER TABLE eh_operational.cost_records ENABLE ROW LEVEL SECURITY;
ALTER TABLE eh_operational.cost_records FORCE ROW LEVEL SECURITY;

CREATE POLICY cost_records_tenant_select ON eh_operational.cost_records
  FOR SELECT TO eh_service
  USING (tenant_id = eh_control.current_tenant_id());

CREATE POLICY cost_records_tenant_insert ON eh_operational.cost_records
  FOR INSERT TO eh_service
  WITH CHECK (tenant_id = eh_control.current_tenant_id());

-- 2.6 proposals — has native tenant_id column.
ALTER TABLE eh_operational.proposals ENABLE ROW LEVEL SECURITY;
ALTER TABLE eh_operational.proposals FORCE ROW LEVEL SECURITY;

CREATE POLICY proposals_tenant_select ON eh_operational.proposals
  FOR SELECT TO eh_service
  USING (tenant_id = eh_control.current_tenant_id());

CREATE POLICY proposals_tenant_insert ON eh_operational.proposals
  FOR INSERT TO eh_service
  WITH CHECK (tenant_id = eh_control.current_tenant_id());

-- ============================================================================
-- Note on eh_admin:
-- eh_admin role does NOT have BYPASSRLS by default. To keep operator inspection
-- unconstrained, the operator can either (a) issue 'SET LOCAL row_security =
-- off' within their admin session, or (b) the operator can choose to ALTER
-- ROLE eh_admin BYPASSRLS (deferred decision — not done here so the deployment
-- defaults to maximum safety). This deliberate choice keeps the door open
-- without committing the project to bypass.
-- ============================================================================
