# db/

Canonical SQL DDL for EventHorizon. The database enforces what these files say — this is the **source of truth** alongside the design narrative in [`../SCHEMA.md`](../SCHEMA.md).

## Layout

```
db/
├── mysql/
│   └── init/
│       └── 01_fvp_schema.sql          # Phase 1 FVP: customers table + eh_admin + eh_service users
└── postgres/
    └── migrations/
        ├── 0001_schemas_and_roles.sql    # eh_control + eh_operational schemas, roles, default privileges
        ├── 0002_identity.sql             # tenants, agents, agent_secrets
        ├── 0003_authorization.sql        # policies, capabilities
        ├── 0004_sources_core.sql         # sources, source_credentials
        ├── 0005_source_kinds.sql         # source_<kind> per-connector typed config tables
        ├── 0006_semantic_schema.sql      # entities, entity_fields + deferred FK from capabilities
        ├── 0007_bindings_routing.sql     # entity_bindings, entity_field_bindings, routing_rules, predicate_nodes
        ├── 0008_entity_relationships.sql # generic typed graph index
        ├── 0009_operational_audit.sql    # audit_log (partitioned)
        ├── 0010_operational_other.sql    # telemetry_events (partitioned), source_health, schema_snapshots, cost_records, proposals
        └── 0011_rls_policies.sql         # RLS on every tenant-scoped table
```

## Workflow (zero-trust §11 Schema-First, Code-After)

1. **Operator reviews and approves the design** in `../SCHEMA.md` and the matching DDL in this directory, per-change.
2. **Operator applies the migration** under the `eh_admin` role:
   - MySQL: `01_fvp_schema.sql` runs at container startup via docker-compose init mount.
   - Postgres: `sqlx migrate run` (or `psql -f`) applied by operator under admin credentials.
3. **The application** authenticates exclusively as `eh_service` thereafter (`SELECT` on `eh_control`, `SELECT + INSERT` on `eh_operational`). Engine refusals are the §12 debugging surface; the application **never** authenticates as admin to bypass them.

## Universal invariants (every table here)

- **UUIDv7** primary keys (`UUID` in PG, `BINARY(16)` in MySQL). App-generated. No string round-trip.
- **INSERT-only writes** — `eh_service` grants exclude UPDATE / DELETE / DDL. Engine-enforced.
- **SCD Type 2 triad** on every state-bearing table (`valid_from`, `valid_to`, `is_current`) — set at insert, never mutated. "Current" computed at query time.
- **Broken-down columns. `JSONB` only for opaque blobs** (raw archived intents, connector-specific event tails). No "metadata" catch-all.
- **Typed FKs and `CHECK` constraints everywhere** — wrong-type writes are engine-refused (architecture §12).
- **RLS on every tenant-scoped table** — defense-in-depth on top of the role grants.

## CI guardrail

`../scripts/check-schema-sync.sh` asserts that the set of tables documented in `../SCHEMA.md` matches the set declared in the DDL files. The set drifts → CI fails → schema-first workflow forces alignment.

## Adding a new table

Per zero-trust §7 + §11: any new table requires explicit, prior, per-change operator approval.

1. Update `../SCHEMA.md` with the new table's columns, types, constraints, indexes.
2. Add a new migration file `db/postgres/migrations/00NN_<topic>.sql` (or the equivalent MySQL init script).
3. Open a PR. Operator reviews **both** files together.
4. After merge, operator applies the migration under `eh_admin`.
5. Application code follows the now-locked schema, authenticating as `eh_service`.

## Adding a new connector kind (e.g., Neo4j, MongoDB)

1. New `source_<kind>` table migration (e.g., `0012_source_neo4j.sql`).
2. Extend the `sources.kind` CHECK enum in the same migration (or a paired one) — operator approves.
3. Publish the connector crate (separate repo or `crates/eh-connector-<kind>/`).
4. Register the connector in `eh-bin` features.

**Zero changes to the kernel** (`eh-core`, `eh-compiler`, `eh-router`).
