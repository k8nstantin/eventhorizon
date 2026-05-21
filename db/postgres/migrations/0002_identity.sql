-- ============================================================================
-- 0002 — Identity & tenancy
-- ============================================================================
-- tenants, agents, agent_secrets — the identity spine.
--
-- Reference: SCHEMA.md §3.1–§3.3.
-- ============================================================================

-- 1. tenants
-- ----------------------------------------------------------------------------

CREATE TABLE eh_control.tenants (
  id                     UUID         NOT NULL,
  name                   TEXT         NOT NULL,
  display_name           TEXT         NOT NULL,
  status                 TEXT         NOT NULL DEFAULT 'active',
  data_db_uri_secret_ref TEXT,

  valid_from             TIMESTAMPTZ  NOT NULL DEFAULT now(),
  valid_to               TIMESTAMPTZ  NOT NULL DEFAULT 'infinity',
  is_current             BOOLEAN      NOT NULL DEFAULT true,

  PRIMARY KEY (id),
  CONSTRAINT tenants_status_chk    CHECK (status IN ('active','suspended','retired')),
  CONSTRAINT tenants_valid_order   CHECK (valid_to >= valid_from)
);

CREATE INDEX tenants_name_current_idx
  ON eh_control.tenants (name)
  WHERE is_current = true;

CREATE INDEX tenants_chain_idx
  ON eh_control.tenants (name, valid_from DESC);

COMMENT ON TABLE eh_control.tenants
  IS 'Tenants. One physical database per tenant for data sources (architecture §5.8). SCD Type 2: INSERT-only, current = ORDER BY valid_from DESC LIMIT 1.';

-- 2. agents
-- ----------------------------------------------------------------------------

CREATE TABLE eh_control.agents (
  id                          UUID         NOT NULL,
  name                        TEXT         NOT NULL,
  tenant_id                   UUID         NOT NULL,
  owner_email                 TEXT         NOT NULL,
  status                      TEXT         NOT NULL DEFAULT 'active',
  cost_budget_cents_per_hour  BIGINT,

  valid_from                  TIMESTAMPTZ  NOT NULL DEFAULT now(),
  valid_to                    TIMESTAMPTZ  NOT NULL DEFAULT 'infinity',
  is_current                  BOOLEAN      NOT NULL DEFAULT true,

  PRIMARY KEY (id),
  CONSTRAINT agents_tenant_fk
    FOREIGN KEY (tenant_id) REFERENCES eh_control.tenants(id),
  CONSTRAINT agents_status_chk
    CHECK (status IN ('active','disabled','retired')),
  CONSTRAINT agents_budget_nonneg
    CHECK (cost_budget_cents_per_hour IS NULL OR cost_budget_cents_per_hour >= 0),
  CONSTRAINT agents_valid_order
    CHECK (valid_to >= valid_from),
  CONSTRAINT agents_owner_email_basic
    CHECK (owner_email LIKE '%@%.%')
);

CREATE INDEX agents_tenant_name_current_idx
  ON eh_control.agents (tenant_id, name)
  WHERE is_current = true;

CREATE INDEX agents_chain_idx
  ON eh_control.agents (tenant_id, name, valid_from DESC);

COMMENT ON TABLE eh_control.agents
  IS 'Logical agents. SCD Type 2. Identity propagates to backends via SET LOCAL app.agent_id (architecture §8).';

-- 3. agent_secrets
-- ----------------------------------------------------------------------------
-- Split from agents so its row-level grants can be tighter (the eh_service
-- account reads this only inside the authn path, never broadly).
-- ----------------------------------------------------------------------------

CREATE TABLE eh_control.agent_secrets (
  id          UUID         NOT NULL,
  agent_id    UUID         NOT NULL,
  token_hash  TEXT         NOT NULL,
  token_kind  TEXT         NOT NULL,
  revoked_at  TIMESTAMPTZ,

  valid_from  TIMESTAMPTZ  NOT NULL DEFAULT now(),
  valid_to    TIMESTAMPTZ  NOT NULL DEFAULT 'infinity',
  is_current  BOOLEAN      NOT NULL DEFAULT true,

  PRIMARY KEY (id),
  CONSTRAINT agent_secrets_agent_fk
    FOREIGN KEY (agent_id) REFERENCES eh_control.agents(id),
  CONSTRAINT agent_secrets_kind_chk
    CHECK (token_kind IN ('static','jwt','mtls_fingerprint')),
  CONSTRAINT agent_secrets_hash_len
    CHECK (length(token_hash) >= 32),
  CONSTRAINT agent_secrets_valid_order
    CHECK (valid_to >= valid_from)
);

CREATE INDEX agent_secrets_agent_active_idx
  ON eh_control.agent_secrets (agent_id)
  WHERE is_current = true AND revoked_at IS NULL;

COMMENT ON TABLE eh_control.agent_secrets
  IS 'Token hashes for agent auth. Revocation is a NEW row with revoked_at set; original row is never updated (zero-trust §10).';
