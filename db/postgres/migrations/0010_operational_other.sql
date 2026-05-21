-- ============================================================================
-- 0010 — Operational: telemetry_events, source_health, schema_snapshots,
--                     cost_records, proposals
-- ============================================================================
-- The unified telemetry stream + supporting event tables. Per architecture
-- §10.1: one canonical stream, dimensioned, never per-connector schemas.
--
-- Reference: SCHEMA.md §4.2 – §4.3.
-- ============================================================================

-- 1. telemetry_events — partitioned monthly
-- ----------------------------------------------------------------------------

CREATE TABLE eh_operational.telemetry_events (
  id               UUID         NOT NULL,
  ts               TIMESTAMPTZ  NOT NULL DEFAULT now(),
  trace_id         UUID         NOT NULL,
  tenant_id        UUID         NOT NULL,
  agent_id         UUID,
  source_id        UUID,
  connector_kind   TEXT,
  entity_id        UUID,
  action           TEXT,
  event_kind       TEXT         NOT NULL,
  status           TEXT,
  latency_ms       INT,
  est_bytes        BIGINT,
  actual_bytes     BIGINT,
  tail             JSONB,

  PRIMARY KEY (ts, id),
  CONSTRAINT telemetry_events_action_chk
    CHECK (action IS NULL OR action IN ('read','append','update','delete')),
  CONSTRAINT telemetry_events_event_kind_chk
    CHECK (event_kind IN (
      'intent_received','intent_authorized','intent_routed',
      'plan_compiled','plan_rejected',
      'execution_started','execution_finished','artifact_emitted',
      'source_health','source_drift','agent_rate_limited'
    )),
  CONSTRAINT telemetry_events_status_chk
    CHECK (status IS NULL OR status IN (
      'ok','denied','over_budget','circuit_open','plan_error','exec_error'
    )),
  CONSTRAINT telemetry_events_connector_kind_chk
    CHECK (connector_kind IS NULL OR connector_kind IN
      ('mysql','postgres','iceberg','duckdb','rag','model','file','snowflake','mssql')),
  CONSTRAINT telemetry_events_latency_nonneg
    CHECK (latency_ms IS NULL OR latency_ms >= 0),
  CONSTRAINT telemetry_events_bytes_nonneg
    CHECK (
      (est_bytes    IS NULL OR est_bytes    >= 0)
      AND (actual_bytes IS NULL OR actual_bytes >= 0)
    )
) PARTITION BY RANGE (ts);

CREATE INDEX telemetry_events_brin_ts_idx
  ON eh_operational.telemetry_events USING BRIN (ts);

CREATE INDEX telemetry_events_tenant_ts_idx
  ON eh_operational.telemetry_events (tenant_id, ts DESC);

CREATE INDEX telemetry_events_agent_kind_idx
  ON eh_operational.telemetry_events (agent_id, event_kind);

CREATE INDEX telemetry_events_connector_kind_idx
  ON eh_operational.telemetry_events (connector_kind, event_kind);

COMMENT ON TABLE eh_operational.telemetry_events
  IS 'Unified telemetry stream — one dimensioned schema for all components, all connectors. Slicing by agent / source / connector_kind / entity / action is a query, not a separate table.';

CREATE TABLE eh_operational.telemetry_events_2026_05
  PARTITION OF eh_operational.telemetry_events
  FOR VALUES FROM ('2026-05-01 00:00:00+00') TO ('2026-06-01 00:00:00+00');

CREATE TABLE eh_operational.telemetry_events_default
  PARTITION OF eh_operational.telemetry_events
  DEFAULT;

-- 2. source_health
-- ----------------------------------------------------------------------------

CREATE TABLE eh_operational.source_health (
  id              UUID         NOT NULL,
  ts              TIMESTAMPTZ  NOT NULL DEFAULT now(),
  source_id       UUID         NOT NULL,
  state           TEXT         NOT NULL,
  latency_ms      INT,
  error_message   TEXT,

  PRIMARY KEY (id),
  CONSTRAINT source_health_state_chk
    CHECK (state IN ('healthy','degraded','unhealthy','unknown')),
  CONSTRAINT source_health_latency_nonneg
    CHECK (latency_ms IS NULL OR latency_ms >= 0)
);

CREATE INDEX source_health_source_ts_idx
  ON eh_operational.source_health (source_id, ts DESC);

CREATE INDEX source_health_brin_ts_idx
  ON eh_operational.source_health USING BRIN (ts);

COMMENT ON TABLE eh_operational.source_health
  IS 'Rolling health snapshots per source. Append-only.';

-- 3. schema_snapshots
-- ----------------------------------------------------------------------------

CREATE TABLE eh_operational.schema_snapshots (
  id               UUID         NOT NULL,
  ts               TIMESTAMPTZ  NOT NULL DEFAULT now(),
  source_id        UUID         NOT NULL,
  schema_payload   JSONB        NOT NULL,
  schema_hash      TEXT         NOT NULL,

  PRIMARY KEY (id),
  CONSTRAINT schema_snapshots_hash_format
    CHECK (schema_hash ~ '^[0-9a-f]{64}$')
);

CREATE INDEX schema_snapshots_source_ts_idx
  ON eh_operational.schema_snapshots (source_id, ts DESC);

CREATE INDEX schema_snapshots_hash_idx
  ON eh_operational.schema_snapshots (source_id, schema_hash);

COMMENT ON TABLE eh_operational.schema_snapshots
  IS 'Physical schema crawled from each source. Drift detector compares latest snapshots to declared schema (Phase 11).';

-- 4. cost_records
-- ----------------------------------------------------------------------------

CREATE TABLE eh_operational.cost_records (
  id                  UUID         NOT NULL,
  ts                  TIMESTAMPTZ  NOT NULL DEFAULT now(),
  tenant_id           UUID         NOT NULL,
  agent_id            UUID         NOT NULL,
  source_id           UUID         NOT NULL,
  est_bytes           BIGINT,
  actual_bytes        BIGINT,
  est_cost_cents      BIGINT,
  actual_cost_cents   BIGINT,

  PRIMARY KEY (id),
  CONSTRAINT cost_records_bytes_nonneg
    CHECK (
      (est_bytes        IS NULL OR est_bytes        >= 0)
      AND (actual_bytes IS NULL OR actual_bytes     >= 0)
    ),
  CONSTRAINT cost_records_cost_nonneg
    CHECK (
      (est_cost_cents    IS NULL OR est_cost_cents    >= 0)
      AND (actual_cost_cents IS NULL OR actual_cost_cents >= 0)
    )
);

CREATE INDEX cost_records_tenant_ts_idx
  ON eh_operational.cost_records (tenant_id, ts DESC);

CREATE INDEX cost_records_agent_ts_idx
  ON eh_operational.cost_records (agent_id, ts DESC);

CREATE INDEX cost_records_source_ts_idx
  ON eh_operational.cost_records (source_id, ts DESC);

CREATE INDEX cost_records_brin_ts_idx
  ON eh_operational.cost_records USING BRIN (ts);

COMMENT ON TABLE eh_operational.cost_records
  IS 'Per-intent cost ledger. Drives cost dashboards and budget enforcement (Phase 9).';

-- 5. proposals
-- ----------------------------------------------------------------------------

CREATE TABLE eh_operational.proposals (
  id                UUID         NOT NULL,
  ts                TIMESTAMPTZ  NOT NULL DEFAULT now(),
  tenant_id         UUID         NOT NULL,
  proposal_kind     TEXT         NOT NULL,
  payload           JSONB        NOT NULL,
  status            TEXT         NOT NULL DEFAULT 'pending',

  PRIMARY KEY (id),
  CONSTRAINT proposals_kind_chk
    CHECK (proposal_kind IN ('materialized_view','routing_rule','cost_optimization','schema_drift_fix')),
  CONSTRAINT proposals_status_chk
    CHECK (status IN ('pending','approved','rejected','applied'))
);

CREATE INDEX proposals_tenant_status_ts_idx
  ON eh_operational.proposals (tenant_id, status, ts DESC);

COMMENT ON TABLE eh_operational.proposals
  IS 'Async copilot output awaiting operator review (Phase 13). Status transitions are append-only — a status change is a NEW row referencing the original via attrs.original_id.';
