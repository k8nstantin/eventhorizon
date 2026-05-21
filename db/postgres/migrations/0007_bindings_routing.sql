-- ============================================================================
-- 0007 — Bindings, routing rules, predicate AST
-- ============================================================================
-- entity_bindings (entity → source), entity_field_bindings (field → column),
-- routing_rules (priority-ordered), predicate_nodes (typed AST).
--
-- Reference: SCHEMA.md §3.10, §3.11, §3.12.
-- ============================================================================

-- 1. entity_bindings
-- ----------------------------------------------------------------------------

CREATE TABLE eh_control.entity_bindings (
  id                       UUID         NOT NULL,
  entity_id                UUID         NOT NULL,
  source_id                UUID         NOT NULL,
  physical_table           TEXT         NOT NULL,
  profile                  TEXT         NOT NULL DEFAULT 'oltp',
  supported_actions        TEXT[]       NOT NULL DEFAULT '{read}',
  lifecycle_state          TEXT         NOT NULL DEFAULT 'bound',
  shadow_traffic_percent   INT          NOT NULL DEFAULT 0,

  valid_from               TIMESTAMPTZ  NOT NULL DEFAULT now(),
  valid_to                 TIMESTAMPTZ  NOT NULL DEFAULT 'infinity',
  is_current               BOOLEAN      NOT NULL DEFAULT true,

  PRIMARY KEY (id),
  CONSTRAINT entity_bindings_entity_fk
    FOREIGN KEY (entity_id) REFERENCES eh_control.entities(id),
  CONSTRAINT entity_bindings_source_fk
    FOREIGN KEY (source_id) REFERENCES eh_control.sources(id),
  CONSTRAINT entity_bindings_profile_chk
    CHECK (profile IN ('oltp','analytical','archival','similarity')),
  CONSTRAINT entity_bindings_lifecycle_chk
    CHECK (lifecycle_state IN ('bound','staging','shadow','production','retired')),
  CONSTRAINT entity_bindings_supported_actions_nonempty
    CHECK (array_length(supported_actions, 1) IS NOT NULL AND array_length(supported_actions, 1) > 0),
  CONSTRAINT entity_bindings_supported_actions_valid
    CHECK (supported_actions <@ ARRAY['read','append','update','delete']::text[]),
  CONSTRAINT entity_bindings_shadow_pct_range
    CHECK (shadow_traffic_percent BETWEEN 0 AND 100),
  CONSTRAINT entity_bindings_table_basic
    CHECK (physical_table ~ '^[A-Za-z_][A-Za-z0-9_]*(\.[A-Za-z_][A-Za-z0-9_]*)?$'),
  CONSTRAINT entity_bindings_valid_order
    CHECK (valid_to >= valid_from)
);

CREATE INDEX entity_bindings_entity_idx
  ON eh_control.entity_bindings (entity_id)
  WHERE is_current = true;

CREATE INDEX entity_bindings_source_idx
  ON eh_control.entity_bindings (source_id)
  WHERE is_current = true;

CREATE INDEX entity_bindings_profile_idx
  ON eh_control.entity_bindings (entity_id, profile)
  WHERE is_current = true;

COMMENT ON TABLE eh_control.entity_bindings
  IS 'Maps a logical entity to a physical table in a specific source. supported_actions is the per-binding capability declaration.';

-- 2. entity_field_bindings
-- ----------------------------------------------------------------------------

CREATE TABLE eh_control.entity_field_bindings (
  id                UUID         NOT NULL,
  binding_id        UUID         NOT NULL,
  entity_field_id   UUID         NOT NULL,
  physical_column   TEXT         NOT NULL,
  transform         TEXT,

  valid_from        TIMESTAMPTZ  NOT NULL DEFAULT now(),
  valid_to          TIMESTAMPTZ  NOT NULL DEFAULT 'infinity',
  is_current        BOOLEAN      NOT NULL DEFAULT true,

  PRIMARY KEY (id),
  CONSTRAINT entity_field_bindings_binding_fk
    FOREIGN KEY (binding_id) REFERENCES eh_control.entity_bindings(id),
  CONSTRAINT entity_field_bindings_field_fk
    FOREIGN KEY (entity_field_id) REFERENCES eh_control.entity_fields(id),
  CONSTRAINT entity_field_bindings_column_basic
    CHECK (physical_column ~ '^[A-Za-z_][A-Za-z0-9_]{0,62}$'),
  CONSTRAINT entity_field_bindings_valid_order
    CHECK (valid_to >= valid_from)
);

CREATE INDEX entity_field_bindings_binding_idx
  ON eh_control.entity_field_bindings (binding_id)
  WHERE is_current = true;

COMMENT ON TABLE eh_control.entity_field_bindings
  IS 'Maps logical entity_fields to physical columns within a binding. Optional transform expression for future use (Phase 7+).';

-- 3. routing_rules
-- ----------------------------------------------------------------------------

CREATE TABLE eh_control.routing_rules (
  id                  UUID         NOT NULL,
  entity_id           UUID         NOT NULL,
  priority            INT          NOT NULL DEFAULT 100,
  target_source_id    UUID         NOT NULL,
  target_binding_id   UUID         NOT NULL,
  predicate_root_id   UUID,

  valid_from          TIMESTAMPTZ  NOT NULL DEFAULT now(),
  valid_to            TIMESTAMPTZ  NOT NULL DEFAULT 'infinity',
  is_current          BOOLEAN      NOT NULL DEFAULT true,

  PRIMARY KEY (id),
  CONSTRAINT routing_rules_entity_fk
    FOREIGN KEY (entity_id) REFERENCES eh_control.entities(id),
  CONSTRAINT routing_rules_source_fk
    FOREIGN KEY (target_source_id) REFERENCES eh_control.sources(id),
  CONSTRAINT routing_rules_binding_fk
    FOREIGN KEY (target_binding_id) REFERENCES eh_control.entity_bindings(id),
  CONSTRAINT routing_rules_priority_nonneg
    CHECK (priority >= 0),
  CONSTRAINT routing_rules_valid_order
    CHECK (valid_to >= valid_from)
  -- predicate_root_id FK is added below after predicate_nodes is created.
);

CREATE INDEX routing_rules_entity_priority_idx
  ON eh_control.routing_rules (entity_id, priority)
  WHERE is_current = true;

COMMENT ON TABLE eh_control.routing_rules
  IS 'Declarative routes from (entity, conditions) → (source, binding). Evaluated in ascending priority. NULL predicate_root_id = unconditional default.';

-- 4. predicate_nodes
-- ----------------------------------------------------------------------------

CREATE TABLE eh_control.predicate_nodes (
  id          UUID         NOT NULL,
  rule_id     UUID         NOT NULL,
  parent_id   UUID,
  position    INT          NOT NULL DEFAULT 0,
  node_kind   TEXT         NOT NULL,
  left_key    TEXT,
  operator    TEXT,
  value_type  TEXT,
  value_text  TEXT,

  valid_from  TIMESTAMPTZ  NOT NULL DEFAULT now(),
  valid_to    TIMESTAMPTZ  NOT NULL DEFAULT 'infinity',
  is_current  BOOLEAN      NOT NULL DEFAULT true,

  PRIMARY KEY (id),
  CONSTRAINT predicate_nodes_rule_fk
    FOREIGN KEY (rule_id) REFERENCES eh_control.routing_rules(id),
  CONSTRAINT predicate_nodes_parent_fk
    FOREIGN KEY (parent_id) REFERENCES eh_control.predicate_nodes(id),
  CONSTRAINT predicate_nodes_kind_chk
    CHECK (node_kind IN ('and','or','not','compare','in','matches')),
  CONSTRAINT predicate_nodes_operator_chk
    CHECK (operator IS NULL OR operator IN ('=','!=','<','<=','>','>=','in','matches')),
  CONSTRAINT predicate_nodes_value_type_chk
    CHECK (value_type IS NULL OR value_type IN ('string','int','duration','enum_list')),
  CONSTRAINT predicate_nodes_position_nonneg
    CHECK (position >= 0),
  CONSTRAINT predicate_nodes_valid_order
    CHECK (valid_to >= valid_from),
  -- Shape constraint: inner nodes (and/or/not) have NULL leaf attrs; leaves
  -- (compare/in/matches) have NOT NULL leaf attrs.
  CONSTRAINT predicate_nodes_shape_chk CHECK (
    (
      node_kind IN ('and','or','not')
      AND left_key   IS NULL
      AND operator   IS NULL
      AND value_type IS NULL
      AND value_text IS NULL
    )
    OR
    (
      node_kind IN ('compare','in','matches')
      AND left_key   IS NOT NULL
      AND operator   IS NOT NULL
      AND value_type IS NOT NULL
      AND value_text IS NOT NULL
    )
  )
);

CREATE INDEX predicate_nodes_rule_idx
  ON eh_control.predicate_nodes (rule_id)
  WHERE is_current = true;

CREATE INDEX predicate_nodes_parent_idx
  ON eh_control.predicate_nodes (parent_id)
  WHERE is_current = true;

COMMENT ON TABLE eh_control.predicate_nodes
  IS 'Routing predicate AST as a parent-child tree. Inner nodes (and/or/not) have child rows; leaves (compare/in/matches) carry typed left/op/value triples. Shape CHECK enforces correctness.';

-- 5. Deferred FK: routing_rules.predicate_root_id → predicate_nodes.id
-- ----------------------------------------------------------------------------

ALTER TABLE eh_control.routing_rules
  ADD CONSTRAINT routing_rules_predicate_root_fk
    FOREIGN KEY (predicate_root_id) REFERENCES eh_control.predicate_nodes(id);
