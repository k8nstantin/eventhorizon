-- ============================================================================
-- 0004 — Sources (kind-agnostic core)
-- ============================================================================
-- sources (the registry) + source_credentials (refs to secrets manager).
-- Per-kind tables are in 0005.
--
-- Reference: SCHEMA.md §3.5, §3.6.
-- ============================================================================

-- 1. sources
-- ----------------------------------------------------------------------------

CREATE TABLE eh_control.sources (
  id              UUID         NOT NULL,
  tenant_id       UUID         NOT NULL,
  name            TEXT         NOT NULL,
  kind            TEXT         NOT NULL,
  ring            TEXT         NOT NULL DEFAULT 'staging',
  status          TEXT         NOT NULL DEFAULT 'registered',
  last_health_at  TIMESTAMPTZ,

  valid_from      TIMESTAMPTZ  NOT NULL DEFAULT now(),
  valid_to        TIMESTAMPTZ  NOT NULL DEFAULT 'infinity',
  is_current      BOOLEAN      NOT NULL DEFAULT true,

  PRIMARY KEY (id),
  CONSTRAINT sources_tenant_fk
    FOREIGN KEY (tenant_id) REFERENCES eh_control.tenants(id),
  CONSTRAINT sources_kind_chk
    CHECK (kind IN ('mysql','postgres','iceberg','duckdb','rag','model','file','snowflake','mssql')),
  CONSTRAINT sources_ring_chk
    CHECK (ring IN ('staging','production','retired')),
  CONSTRAINT sources_status_chk
    CHECK (status IN ('registered','probed','bound','staging','production','disabled','error')),
  CONSTRAINT sources_valid_order
    CHECK (valid_to >= valid_from)
);

CREATE INDEX sources_tenant_name_current_idx
  ON eh_control.sources (tenant_id, name)
  WHERE is_current = true;

CREATE INDEX sources_kind_status_idx
  ON eh_control.sources (kind, status)
  WHERE is_current = true;

CREATE INDEX sources_chain_idx
  ON eh_control.sources (tenant_id, name, valid_from DESC);

COMMENT ON TABLE eh_control.sources
  IS 'Registered backends, kind-agnostic columns. Per-kind typed config lives in source_<kind> tables — the ProxySQL pattern (architecture §5.3, §5.6).';
COMMENT ON COLUMN eh_control.sources.kind
  IS 'Extending this enum is an operator-approved migration (zero-trust §7, §11).';

-- 2. source_credentials
-- ----------------------------------------------------------------------------
-- Refs only. Values NEVER stored here.
-- ----------------------------------------------------------------------------

CREATE TABLE eh_control.source_credentials (
  id          UUID         NOT NULL,
  source_id   UUID         NOT NULL,
  purpose     TEXT         NOT NULL,
  secret_ref  TEXT         NOT NULL,

  valid_from  TIMESTAMPTZ  NOT NULL DEFAULT now(),
  valid_to    TIMESTAMPTZ  NOT NULL DEFAULT 'infinity',
  is_current  BOOLEAN      NOT NULL DEFAULT true,

  PRIMARY KEY (id),
  CONSTRAINT source_credentials_source_fk
    FOREIGN KEY (source_id) REFERENCES eh_control.sources(id),
  CONSTRAINT source_credentials_purpose_chk
    CHECK (purpose IN ('username','password','token','tls_cert','tls_key','api_key')),
  CONSTRAINT source_credentials_ref_scheme
    CHECK (secret_ref ~ '^(vault|k8s|aws-sm|gcp-sm|file|env)://'),
  CONSTRAINT source_credentials_valid_order
    CHECK (valid_to >= valid_from)
);

CREATE INDEX source_credentials_source_idx
  ON eh_control.source_credentials (source_id, purpose)
  WHERE is_current = true;

COMMENT ON TABLE eh_control.source_credentials
  IS 'References to secrets manager (Vault, k8s Secret, AWS/GCP Secrets Manager). VALUES are NEVER stored.';
