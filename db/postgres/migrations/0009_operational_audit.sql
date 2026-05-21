-- ============================================================================
-- 0009 — Operational: audit_log (durable per-intent record)
-- ============================================================================
-- Time-partitioned monthly. Append-only. The PG audit sink writes one row per
-- intent transactionally before the HTTP response returns. If audit write
-- fails, the request fails (architecture §10).
--
-- Reference: SCHEMA.md §4.1.
-- ============================================================================

-- 1. Partitioned parent
-- ----------------------------------------------------------------------------

CREATE TABLE eh_operational.audit_log (
  id               UUID         NOT NULL,
  ts               TIMESTAMPTZ  NOT NULL DEFAULT now(),
  trace_id         UUID         NOT NULL,
  tenant_id        UUID         NOT NULL,
  agent_id         UUID         NOT NULL,
  entity_id        UUID,
  source_id        UUID,
  connector_kind   TEXT,
  action           TEXT         NOT NULL,
  plan_hash        TEXT,
  decision         TEXT         NOT NULL,
  outcome          TEXT         NOT NULL,
  latency_ms       INT          NOT NULL,
  est_bytes        BIGINT,
  actual_bytes     BIGINT,
  raw_intent       JSONB        NOT NULL,

  PRIMARY KEY (ts, id),    -- partition key first per PG requirement
  CONSTRAINT audit_log_action_chk
    CHECK (action IN ('read','append','update','delete')),
  CONSTRAINT audit_log_decision_chk
    CHECK (decision IN ('permit','deny')),
  CONSTRAINT audit_log_outcome_chk
    CHECK (outcome IN ('ok','denied','over_budget','circuit_open','plan_error','exec_error')),
  CONSTRAINT audit_log_connector_kind_chk
    CHECK (connector_kind IS NULL OR connector_kind IN
      ('mysql','postgres','iceberg','duckdb','rag','model','file','snowflake','mssql')),
  CONSTRAINT audit_log_latency_nonneg
    CHECK (latency_ms >= 0),
  CONSTRAINT audit_log_bytes_nonneg
    CHECK (
      (est_bytes    IS NULL OR est_bytes    >= 0)
      AND (actual_bytes IS NULL OR actual_bytes >= 0)
    ),
  CONSTRAINT audit_log_plan_hash_format
    CHECK (plan_hash IS NULL OR plan_hash ~ '^[0-9a-f]{64}$')
) PARTITION BY RANGE (ts);

COMMENT ON TABLE eh_operational.audit_log
  IS 'Durable per-intent audit record. Time-partitioned monthly. Append-only. PG audit sink writes one row transactionally; audit failure fails the request.';

CREATE INDEX audit_log_brin_ts_idx
  ON eh_operational.audit_log USING BRIN (ts);

CREATE INDEX audit_log_tenant_ts_idx
  ON eh_operational.audit_log (tenant_id, ts DESC);

CREATE INDEX audit_log_trace_idx
  ON eh_operational.audit_log (trace_id);

-- 2. Bootstrap partition (current month) and a default catch-all
-- ----------------------------------------------------------------------------
-- The operator (or a scheduled job) creates next month's partition before
-- it's needed. The default partition catches anything that lands outside
-- declared ranges (should be rare; surfaces config drift).
--
-- Bootstrap partition for May 2026 (current month in this initial commit).
-- ----------------------------------------------------------------------------

CREATE TABLE eh_operational.audit_log_2026_05
  PARTITION OF eh_operational.audit_log
  FOR VALUES FROM ('2026-05-01 00:00:00+00') TO ('2026-06-01 00:00:00+00');

CREATE TABLE eh_operational.audit_log_default
  PARTITION OF eh_operational.audit_log
  DEFAULT;

COMMENT ON TABLE eh_operational.audit_log_default
  IS 'Catch-all partition. Rows landing here indicate missing forward partition — alert and roll a new partition.';
