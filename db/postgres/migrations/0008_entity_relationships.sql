-- ============================================================================
-- 0008 — Entity relationships (the explicit graph index)
-- ============================================================================
-- Generic typed edges between any two control-plane entities. Indexes the
-- graph that the FKs in 0002–0007 already form, so traversal queries
-- ("what depends on X?", "what's downstream of Y?") become single SQL queries
-- alongside the typed per-kind tables.
--
-- Reference: SCHEMA.md §3.13. Architecture §5.13.
-- ============================================================================

CREATE TABLE eh_control.entity_relationships (
  id              UUID         NOT NULL,
  from_id         UUID         NOT NULL,
  from_kind       TEXT         NOT NULL,
  to_id           UUID         NOT NULL,
  to_kind         TEXT         NOT NULL,
  relation_kind   TEXT         NOT NULL,
  attrs           JSONB,

  valid_from      TIMESTAMPTZ  NOT NULL DEFAULT now(),
  valid_to        TIMESTAMPTZ  NOT NULL DEFAULT 'infinity',
  is_current      BOOLEAN      NOT NULL DEFAULT true,

  PRIMARY KEY (id),
  CONSTRAINT entity_relationships_from_kind_chk
    CHECK (from_kind IN ('agent','source','entity','binding','policy','capability','rule','connector')),
  CONSTRAINT entity_relationships_to_kind_chk
    CHECK (to_kind   IN ('agent','source','entity','binding','policy','capability','rule','connector')),
  CONSTRAINT entity_relationships_relation_kind_chk
    CHECK (relation_kind IN ('depends_on','routes_to','owns','extends','authorizes','derives_from','observes')),
  CONSTRAINT entity_relationships_not_self
    CHECK (from_id <> to_id OR from_kind <> to_kind),
  CONSTRAINT entity_relationships_valid_order
    CHECK (valid_to >= valid_from)
);

CREATE INDEX entity_relationships_from_idx
  ON eh_control.entity_relationships (from_id, relation_kind)
  WHERE is_current = true;

CREATE INDEX entity_relationships_to_idx
  ON eh_control.entity_relationships (to_id, relation_kind)
  WHERE is_current = true;

CREATE INDEX entity_relationships_kind_pair_idx
  ON eh_control.entity_relationships (from_kind, to_kind, relation_kind)
  WHERE is_current = true;

COMMENT ON TABLE eh_control.entity_relationships
  IS 'Explicit graph index of typed edges between any two control-plane entities. Indexes the FK graph; does NOT replace typed per-kind tables. New relation kinds = operator-approved migration extending the CHECK enum.';

COMMENT ON COLUMN eh_control.entity_relationships.attrs
  IS 'Narrow JSONB tail for relation-specific extras only. Cross-cutting fields stay typed.';
