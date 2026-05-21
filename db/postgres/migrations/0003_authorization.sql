-- ============================================================================
-- 0003 — Authorization
-- ============================================================================
-- policies (Cedar policy text), capabilities ((agent, entity, action) grants).
--
-- Reference: SCHEMA.md §3.3, §3.4. Architecture §8.
-- ============================================================================

-- 1. policies
-- ----------------------------------------------------------------------------

CREATE TABLE eh_control.policies (
  id              UUID         NOT NULL,
  tenant_id       UUID         NOT NULL,
  name            TEXT         NOT NULL,
  body            TEXT         NOT NULL,
  compiled_hash   TEXT         NOT NULL,

  valid_from      TIMESTAMPTZ  NOT NULL DEFAULT now(),
  valid_to        TIMESTAMPTZ  NOT NULL DEFAULT 'infinity',
  is_current      BOOLEAN      NOT NULL DEFAULT true,

  PRIMARY KEY (id),
  CONSTRAINT policies_tenant_fk
    FOREIGN KEY (tenant_id) REFERENCES eh_control.tenants(id),
  CONSTRAINT policies_body_nonempty
    CHECK (length(body) > 0),
  CONSTRAINT policies_hash_sha256
    CHECK (compiled_hash ~ '^[0-9a-f]{64}$'),
  CONSTRAINT policies_valid_order
    CHECK (valid_to >= valid_from)
);

CREATE INDEX policies_tenant_name_current_idx
  ON eh_control.policies (tenant_id, name)
  WHERE is_current = true;

CREATE INDEX policies_chain_idx
  ON eh_control.policies (tenant_id, name, valid_from DESC);

COMMENT ON TABLE eh_control.policies
  IS 'Cedar policy text, version-tagged. compiled_hash is sha256 of the Cedar AST for cache invalidation.';

-- 2. capabilities
-- ----------------------------------------------------------------------------
-- (agent, entity, action) grants. Optional condition_policy_id references a
-- Cedar policy that further restricts the grant (e.g. "only for tenant_id =
-- the agent's tenant" or "only when context.window <= 90d").
-- ----------------------------------------------------------------------------

-- Note: entities table is defined in migration 0006; we use a NOT VALID FK
-- here for ordering, then VALIDATE in 0006. Postgres requires the referenced
-- table to exist at CREATE time, so the FK is added in 0006 after entities
-- is in place. For now, entity_id is an unconstrained UUID column; the FK
-- constraint is attached in 0006.

CREATE TABLE eh_control.capabilities (
  id                    UUID         NOT NULL,
  agent_id              UUID         NOT NULL,
  entity_id             UUID         NOT NULL,
  action                TEXT         NOT NULL,
  condition_policy_id   UUID,

  valid_from            TIMESTAMPTZ  NOT NULL DEFAULT now(),
  valid_to              TIMESTAMPTZ  NOT NULL DEFAULT 'infinity',
  is_current            BOOLEAN      NOT NULL DEFAULT true,

  PRIMARY KEY (id),
  CONSTRAINT capabilities_agent_fk
    FOREIGN KEY (agent_id) REFERENCES eh_control.agents(id),
  CONSTRAINT capabilities_policy_fk
    FOREIGN KEY (condition_policy_id) REFERENCES eh_control.policies(id),
  CONSTRAINT capabilities_action_chk
    CHECK (action IN ('read','append','update','delete')),
  CONSTRAINT capabilities_valid_order
    CHECK (valid_to >= valid_from)
);

CREATE INDEX capabilities_agent_current_idx
  ON eh_control.capabilities (agent_id)
  WHERE is_current = true;

CREATE INDEX capabilities_entity_action_idx
  ON eh_control.capabilities (entity_id, action)
  WHERE is_current = true;

COMMENT ON TABLE eh_control.capabilities
  IS '(agent, entity, action) grants. Cedar policy referenced via condition_policy_id can further restrict; absent = unconditional grant.';
