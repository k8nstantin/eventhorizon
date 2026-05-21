-- ============================================================================
-- 0006 — Semantic schema
-- ============================================================================
-- entities (agent-facing concepts) + entity_fields (their typed attributes).
-- Also adds the FK from capabilities(entity_id) → entities(id), deferred from
-- 0003 because of table ordering.
--
-- Reference: SCHEMA.md §3.8, §3.9.
-- ============================================================================

-- 1. entities
-- ----------------------------------------------------------------------------

CREATE TABLE eh_control.entities (
  id              UUID         NOT NULL,
  tenant_id       UUID         NOT NULL,
  kind            TEXT         NOT NULL DEFAULT 'data',
  name            TEXT         NOT NULL,
  description     TEXT,
  entity_version  INT          NOT NULL DEFAULT 1,

  valid_from      TIMESTAMPTZ  NOT NULL DEFAULT now(),
  valid_to        TIMESTAMPTZ  NOT NULL DEFAULT 'infinity',
  is_current      BOOLEAN      NOT NULL DEFAULT true,

  PRIMARY KEY (id),
  CONSTRAINT entities_tenant_fk
    FOREIGN KEY (tenant_id) REFERENCES eh_control.tenants(id),
  CONSTRAINT entities_kind_chk
    CHECK (kind IN ('data','control','derived','event')),
  CONSTRAINT entities_version_pos
    CHECK (entity_version >= 1),
  CONSTRAINT entities_name_basic
    CHECK (name ~ '^[A-Za-z][A-Za-z0-9_]{0,62}$'),
  CONSTRAINT entities_valid_order
    CHECK (valid_to >= valid_from)
);

CREATE INDEX entities_tenant_name_current_idx
  ON eh_control.entities (tenant_id, name)
  WHERE is_current = true;

CREATE INDEX entities_kind_idx
  ON eh_control.entities (kind)
  WHERE is_current = true;

CREATE INDEX entities_chain_idx
  ON eh_control.entities (tenant_id, name, valid_from DESC);

COMMENT ON TABLE eh_control.entities
  IS 'Agent-facing semantic entities. kind=''data'' for tenant data entities (Customer, Order); kind=''control'' for control-plane objects exposed via admin (Source, Binding); kind=''derived'' for materialised views; kind=''event'' for stream-shaped concepts.';

-- 2. entity_fields
-- ----------------------------------------------------------------------------

CREATE TABLE eh_control.entity_fields (
  id          UUID         NOT NULL,
  entity_id   UUID         NOT NULL,
  name        TEXT         NOT NULL,
  data_type   TEXT         NOT NULL,
  nullable    BOOLEAN      NOT NULL DEFAULT false,
  pii_flag    BOOLEAN      NOT NULL DEFAULT false,
  position    INT          NOT NULL DEFAULT 0,

  valid_from  TIMESTAMPTZ  NOT NULL DEFAULT now(),
  valid_to    TIMESTAMPTZ  NOT NULL DEFAULT 'infinity',
  is_current  BOOLEAN      NOT NULL DEFAULT true,

  PRIMARY KEY (id),
  CONSTRAINT entity_fields_entity_fk
    FOREIGN KEY (entity_id) REFERENCES eh_control.entities(id),
  CONSTRAINT entity_fields_data_type_chk
    CHECK (data_type IN ('string','text','int','bigint','decimal','float','bool','uuid','timestamp','json','binary')),
  CONSTRAINT entity_fields_name_basic
    CHECK (name ~ '^[a-z][a-z0-9_]{0,62}$'),
  CONSTRAINT entity_fields_position_nonneg
    CHECK (position >= 0),
  CONSTRAINT entity_fields_valid_order
    CHECK (valid_to >= valid_from)
);

CREATE INDEX entity_fields_entity_position_idx
  ON eh_control.entity_fields (entity_id, position)
  WHERE is_current = true;

CREATE INDEX entity_fields_entity_name_current_idx
  ON eh_control.entity_fields (entity_id, name)
  WHERE is_current = true;

COMMENT ON TABLE eh_control.entity_fields
  IS 'Typed field declarations per entity. Sub-entity of entities.';

-- 3. Deferred FK: capabilities.entity_id → entities.id
-- ----------------------------------------------------------------------------

ALTER TABLE eh_control.capabilities
  ADD CONSTRAINT capabilities_entity_fk
    FOREIGN KEY (entity_id) REFERENCES eh_control.entities(id);
