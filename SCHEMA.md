# EventHorizon — Schema Reference

> **Status: DRAFT.** Per [zero-trust §11](./.claude/skills/zero-trust-execution/SKILL.md) (Schema-First, Code-After) every table, column, index, constraint, and seed row defined here is subject to **explicit, per-change operator approval** before being applied to a live store. This document is the design contract; migrations follow operator approval, code follows migrations.

This document is the load-bearing schema reference. It defines:

- The universal data discipline that every table obeys.
- The two-logical-schema split inside the control-plane Postgres database (`eh_control`, `eh_operational`).
- Per-kind connector tables (the ProxySQL pattern).
- The entity-relationships graph index.
- The routing predicate AST.
- The account & grant model.
- The data-source conventions for tenant databases (MySQL Phase 1, Postgres Phase 4+, Iceberg Phase 7+).
- Migration & drift-detection workflow.

A rendered browser-friendly version is available at [schema.html](./schema.html) (served via GitHub Pages alongside the architecture doc).

Cross-references in this document:

- `architecture §N` → see [eventhorizon_architecture.md](./eventhorizon_architecture.md).
- `zero-trust §N` → see [.claude/skills/zero-trust-execution/SKILL.md](./.claude/skills/zero-trust-execution/SKILL.md).

---

## 1. Universal data discipline

Every persistent store the system writes to — control plane, operational plane, and tenant data — obeys the same five invariants. These are non-negotiable. They appear in `architecture §5.1` and are binding here.

### 1.1 UUIDv7 primary keys

- All primary keys are **UUIDv7** (RFC 9562 v7, time-ordered). Generated via the `uuid7` crate in Rust.
- Postgres storage: native `UUID` type.
- MySQL storage: `BINARY(16) NOT NULL`. Bound to `uuid::Uuid` directly via `sqlx`; **no `UUID_TO_BIN()` / `BIN_TO_UUID()` shim** in application code (`zero-trust §14`).
- Auto-increment integer PKs are forbidden.

### 1.2 INSERT-only writes

- No `UPDATE`. No `DELETE`. No `UPSERT`.
- The application service account's grants make this **engine-enforced** (`zero-trust §10`, `§13`).
- "Current" state is computed at query time — never by mutating prior rows (`§4`).

### 1.3 SCD Type 2 triad

Every state-bearing table carries the same three columns:

| Column | Type | Default | Mutability |
|---|---|---|---|
| `valid_from` | `TIMESTAMPTZ NOT NULL` | `now()` at insert | set at insert, **never mutated** |
| `valid_to`   | `TIMESTAMPTZ NOT NULL` | `'infinity'` at insert | set at insert, **never mutated** |
| `is_current` | `BOOLEAN     NOT NULL` | `true` at insert       | set at insert, **never mutated** |

The "close-prior + insert-new" pattern is forbidden (`zero-trust §10`). New versions are pure INSERTs; "current" is computed per chain key at query time.

### 1.4 Time-travel by query, not by mutation

```sql
-- "Latest version of agent X"
SELECT *
FROM eh_control.agents
WHERE name = $1
ORDER BY valid_from DESC
LIMIT 1;

-- "Version of agent X as of timestamp T"
SELECT *
FROM eh_control.agents
WHERE name = $1
  AND valid_from <= $2
  AND (valid_to IS NULL OR valid_to > $2)
ORDER BY valid_from DESC
LIMIT 1;

-- "Historical valid_to for any version (computed)"
SELECT
  *,
  LEAD(valid_from) OVER (PARTITION BY name ORDER BY valid_from) AS effective_valid_to
FROM eh_control.agents
WHERE name = $1;
```

The `valid_to` column stored at insert is advisory (always `'infinity'` by default). Effective `valid_to` is the `LEAD(valid_from)` of the next row in the chain.

### 1.5 Broken-down columns. `JSONB` only for opaque blobs

Per `zero-trust §14`. Every named attribute is a typed column. Typed FKs everywhere. `CHECK` constraints on every status / enum column. `JSONB` is reserved for genuinely opaque payloads:

| Use case | Storage |
|---|---|
| Agent identity, source config, entity fields, routing rules | **typed columns** |
| Connector kind-specific config | **per-kind typed table** (§4) — never JSONB |
| Routing predicates | **AST nodes in `predicate_nodes` table** (§5) — never JSONB |
| Cedar policy text | single `TEXT` column (it's its own grammar) |
| Raw `Intent` archived in `audit_log` for replay | **`JSONB`** (we never query into it) |
| Connector-specific tail metadata on telemetry events | **narrow `JSONB`** (only the kind-emitted extras; all cross-cutting fields stay typed) |
| Cost / latency / byte counts | **typed numeric** |

A `metadata JSONB` catch-all column on any config table is **forbidden**.

---

## 2. The two logical schemas

The control-plane Postgres database holds two schemas with deliberately different grant axes, retention, and access patterns. See `architecture §5.2`.

| Schema | Purpose | Size | `eh_service` grant | Backup priority |
|---|---|---|---|---|
| `eh_control` | Config: agents, sources, entities, bindings, rules, policies | small | `SELECT` only | high — source of truth |
| `eh_operational` | Events: audit log, telemetry, health, drift, cost, proposals | large | `SELECT` + `INSERT` only | rebuildable from Kafka mirror |

---

## 3. `eh_control` schema

Below are the tables in `eh_control`. Column lists are the design contract; each table's full DDL is committed in a migration file under operator review per `zero-trust §11`.

### 3.1 `agents`

Identity. One row per logical agent (per version, SCD Type 2).

| Column | Type | Notes |
|---|---|---|
| `id` | `UUID PRIMARY KEY` | UUIDv7 |
| `name` | `TEXT NOT NULL` | unique chain key |
| `tenant_id` | `UUID NOT NULL` | FK → tenant registry (Phase 6+) |
| `owner_email` | `TEXT NOT NULL` | who is accountable |
| `status` | `TEXT NOT NULL CHECK (status IN ('active','disabled','retired'))` | |
| `cost_budget_cents_per_hour` | `BIGINT NULL` | nullable = no budget |
| `valid_from`, `valid_to`, `is_current` | SCD Type 2 triad | per §1.3 |

### 3.2 `agent_secrets`

Token hashes — split into a separate table so its grants can be tighter than `agents`.

| Column | Type | Notes |
|---|---|---|
| `id` | `UUID PRIMARY KEY` | UUIDv7 |
| `agent_id` | `UUID NOT NULL REFERENCES agents(id)` | FK |
| `token_hash` | `TEXT NOT NULL` | argon2 / bcrypt — never the raw token |
| `token_kind` | `TEXT NOT NULL CHECK (token_kind IN ('static','jwt','mtls_fingerprint'))` | |
| `revoked_at` | `TIMESTAMPTZ NULL` | revocation is a new row with this set; original row untouched |
| SCD Type 2 triad | | |

### 3.3 `capabilities`

`(agent, entity, action)` grants.

| Column | Type | Notes |
|---|---|---|
| `id` | `UUID PRIMARY KEY` | UUIDv7 |
| `agent_id` | `UUID NOT NULL REFERENCES agents(id)` | |
| `entity_id` | `UUID NOT NULL REFERENCES entities(id)` | |
| `action` | `TEXT NOT NULL CHECK (action IN ('read','append','update','delete'))` | |
| `condition_policy_id` | `UUID NULL REFERENCES policies(id)` | optional Cedar policy reference |
| SCD Type 2 triad | | |

### 3.4 `policies`

Cedar policy text, version-tagged.

| Column | Type | Notes |
|---|---|---|
| `id` | `UUID PRIMARY KEY` | UUIDv7 |
| `name` | `TEXT NOT NULL` | chain key |
| `body` | `TEXT NOT NULL` | Cedar source |
| `compiled_hash` | `TEXT NOT NULL` | sha256 of Cedar AST for cache invalidation |
| SCD Type 2 triad | | |

### 3.5 `sources`

Registered backends, kind-agnostic columns.

| Column | Type | Notes |
|---|---|---|
| `id` | `UUID PRIMARY KEY` | UUIDv7 |
| `name` | `TEXT NOT NULL` | chain key (e.g. `pg_oltp_prod`) |
| `kind` | `TEXT NOT NULL CHECK (kind IN ('mysql','postgres','iceberg','duckdb','rag','model','file','snowflake','mssql'))` | one row per connector kind we register |
| `ring` | `TEXT NOT NULL CHECK (ring IN ('staging','production','retired'))` | |
| `status` | `TEXT NOT NULL CHECK (status IN ('registered','probed','bound','staging','production','disabled','error'))` | lifecycle |
| `last_health_at` | `TIMESTAMPTZ NULL` | populated by background health checker |
| SCD Type 2 triad | | |

### 3.6 `source_credentials`

References to secrets manager — **values are never stored here**.

| Column | Type | Notes |
|---|---|---|
| `id` | `UUID PRIMARY KEY` | UUIDv7 |
| `source_id` | `UUID NOT NULL REFERENCES sources(id)` | |
| `secret_ref` | `TEXT NOT NULL` | URI: `vault://…`, `k8s://…`, `aws-sm://…` |
| `purpose` | `TEXT NOT NULL CHECK (purpose IN ('username','password','token','tls_cert','tls_key'))` | |
| SCD Type 2 triad | | |

### 3.7 `source_<kind>` per-connector tables (the ProxySQL pattern)

Each registered connector kind owns its own typed config table. FK 1:1 to `sources`. The kernel and connector registry know how to read each one.

#### 3.7.1 `source_mysql` (Phase 1 worked example)

```sql
CREATE TABLE eh_control.source_mysql (
  source_id            UUID PRIMARY KEY REFERENCES eh_control.sources(id),
  host                 TEXT NOT NULL,
  port                 INT  NOT NULL CHECK (port BETWEEN 1 AND 65535),
  database_name        TEXT NOT NULL,
  username_secret_ref  TEXT NOT NULL,
  password_secret_ref  TEXT NOT NULL,
  ssl_mode             TEXT NOT NULL CHECK (ssl_mode IN ('disabled','preferred','required','verify_ca','verify_identity')),
  max_pool_size        INT  NOT NULL DEFAULT 8 CHECK (max_pool_size > 0),
  valid_from           TIMESTAMPTZ NOT NULL DEFAULT now(),
  valid_to             TIMESTAMPTZ NOT NULL DEFAULT 'infinity',
  is_current           BOOLEAN     NOT NULL DEFAULT true
);
```

#### 3.7.2 Other connector kinds (forward-looking)

- `source_postgres` — same shape as `source_mysql` with `application_name`, `search_path` columns.
- `source_iceberg` — `catalog_uri`, `namespace`, `warehouse`, `auth_kind` columns.
- `source_duckdb` — `database_path` (or `:memory:`), `extensions[]` text array.
- `source_rag` — `vector_store_uri`, `embedding_model`, `top_k_default`.
- `source_model` — `provider`, `model_id`, `api_key_secret_ref`, `max_tokens_default`.
- `source_file` — `root_path`, `format`, `partition_pattern`.
- `source_snowflake` — `account`, `warehouse`, `database`, `auth_kind` (ADBC-backed).
- `source_mssql` — `host`, `port`, `instance`, `database`, `auth_kind` (`tiberius`-backed).

Each is a separate migration, operator-approved per `§11`.

### 3.8 `entities`

Logical entity definitions. The semantic surface agents see.

| Column | Type | Notes |
|---|---|---|
| `id` | `UUID PRIMARY KEY` | UUIDv7 |
| `name` | `TEXT NOT NULL` | chain key (e.g. `Customer`) |
| `tenant_id` | `UUID NOT NULL` | tenant-scoped definitions |
| `description` | `TEXT NULL` | |
| `version` | `INT NOT NULL DEFAULT 1` | bumped per breaking change |
| SCD Type 2 triad | | |

### 3.9 `entity_fields`

Per-field type declarations. Sub-entity of `entities`.

| Column | Type | Notes |
|---|---|---|
| `id` | `UUID PRIMARY KEY` | UUIDv7 |
| `entity_id` | `UUID NOT NULL REFERENCES entities(id)` | parent |
| `name` | `TEXT NOT NULL` | chain key within entity |
| `data_type` | `TEXT NOT NULL CHECK (data_type IN ('string','text','int','bigint','decimal','float','bool','uuid','timestamp','json','binary'))` | |
| `nullable` | `BOOLEAN NOT NULL DEFAULT false` | |
| `pii_flag` | `BOOLEAN NOT NULL DEFAULT false` | informational tag for compliance |
| SCD Type 2 triad | | |

### 3.10 `entity_bindings`

Maps a logical entity (and its fields) to a physical table in a specific source.

| Column | Type | Notes |
|---|---|---|
| `id` | `UUID PRIMARY KEY` | UUIDv7 |
| `entity_id` | `UUID NOT NULL REFERENCES entities(id)` | |
| `source_id` | `UUID NOT NULL REFERENCES sources(id)` | |
| `physical_table` | `TEXT NOT NULL` | e.g. `public.customers` or `warehouse.customers_history` |
| `profile` | `TEXT NOT NULL CHECK (profile IN ('oltp','analytical','archival','similarity'))` | |
| `supported_actions` | `TEXT[] NOT NULL CHECK (array_length(supported_actions, 1) > 0)` | per-binding capability declaration |
| SCD Type 2 triad | | |

Sub-table `entity_field_bindings(binding_id, entity_field_id, physical_column)` maps logical fields to physical columns.

### 3.11 `routing_rules`

Declarative routes from `(entity, action, predicate)` to `(source, binding)`.

| Column | Type | Notes |
|---|---|---|
| `id` | `UUID PRIMARY KEY` | UUIDv7 |
| `entity_id` | `UUID NOT NULL REFERENCES entities(id)` | |
| `priority` | `INT  NOT NULL` | lower = higher priority; ties resolved by `valid_from` |
| `target_source_id` | `UUID NOT NULL REFERENCES sources(id)` | |
| `target_binding_id` | `UUID NOT NULL REFERENCES entity_bindings(id)` | |
| `predicate_root_id` | `UUID NULL REFERENCES predicate_nodes(id)` | NULL = unconditional default |
| SCD Type 2 triad | | |

### 3.12 `predicate_nodes` — routing predicate AST

Per `architecture §5.7`. Parent-child tree of boolean expression nodes.

```sql
CREATE TABLE eh_control.predicate_nodes (
  id            UUID PRIMARY KEY,                        -- UUIDv7
  rule_id       UUID NOT NULL REFERENCES routing_rules(id),
  parent_id     UUID NULL REFERENCES predicate_nodes(id),
  position      INT  NOT NULL DEFAULT 0,                 -- sibling ordering
  node_kind     TEXT NOT NULL CHECK (node_kind IN ('and','or','not','compare','in','matches')),
  left_key      TEXT NULL,                               -- e.g. 'action', 'mode', 'window'
  operator      TEXT NULL CHECK (operator IN ('=','!=','<','<=','>','>=','in','matches')),
  value_type    TEXT NULL CHECK (value_type IN ('string','int','duration','enum_list')),
  value_text    TEXT NULL,
  valid_from    TIMESTAMPTZ NOT NULL DEFAULT now(),
  valid_to      TIMESTAMPTZ NOT NULL DEFAULT 'infinity',
  is_current    BOOLEAN     NOT NULL DEFAULT true,
  CONSTRAINT predicate_shape CHECK (
    (node_kind IN ('and','or','not') AND left_key IS NULL AND operator IS NULL AND value_text IS NULL)
    OR
    (node_kind IN ('compare','in','matches') AND left_key IS NOT NULL AND operator IS NOT NULL AND value_text IS NOT NULL)
  )
);
```

Inner nodes (`and`/`or`/`not`) have children referenced via `parent_id`; leaves (`compare`/`in`/`matches`) carry typed left/op/value triples. Evaluator walks the tree recursively. Phase 1 FVP routing is unconditional defaults (no predicate trees yet); the AST machinery lands when the YAML config gains conditional routing in Phase 1–2 polish.

### 3.13 `entity_relationships` — the explicit graph index

Per `architecture §5.13`. Generic typed edges between any two control-plane entities, enabling graph-traversal queries alongside the typed per-kind tables.

```sql
CREATE TABLE eh_control.entity_relationships (
  id              UUID PRIMARY KEY,                            -- UUIDv7
  from_id         UUID NOT NULL,
  from_kind       TEXT NOT NULL CHECK (from_kind IN
                    ('agent','source','entity','binding','policy','capability','rule','connector')),
  to_id           UUID NOT NULL,
  to_kind         TEXT NOT NULL CHECK (to_kind   IN
                    ('agent','source','entity','binding','policy','capability','rule','connector')),
  relation_kind   TEXT NOT NULL CHECK (relation_kind IN
                    ('depends_on','routes_to','owns','extends','authorizes','derives_from','observes')),
  attrs           JSONB,
  valid_from      TIMESTAMPTZ NOT NULL DEFAULT now(),
  valid_to        TIMESTAMPTZ NOT NULL DEFAULT 'infinity',
  is_current      BOOLEAN     NOT NULL DEFAULT true
);
CREATE INDEX ON eh_control.entity_relationships (from_id, relation_kind);
CREATE INDEX ON eh_control.entity_relationships (to_id, relation_kind);
```

This table **does not replace** the per-kind typed tables — it indexes the graph that the FKs already form. New relation kinds are operator-approved migrations that extend the `CHECK` enum.

---

## 4. `eh_operational` schema

Append-only event store. Large, hot, rebuildable from the Kafka mirror downstream.

### 4.1 `audit_log`

Durable per-intent record. Written transactionally inside the request path; if write fails, the request fails (`architecture §10`).

| Column | Type | Notes |
|---|---|---|
| `id` | `UUID PRIMARY KEY` | UUIDv7, sortable by time |
| `trace_id` | `UUID NOT NULL` | OTel trace id |
| `tenant_id` | `UUID NOT NULL` | for RLS |
| `agent_id` | `UUID NOT NULL` | |
| `entity_id` | `UUID NULL` | |
| `source_id` | `UUID NULL` | NULL on denial / error before routing |
| `connector_kind` | `TEXT NULL` | mirror of `sources.kind` for fast filtering without join |
| `action` | `TEXT NOT NULL CHECK (action IN ('read','append','update','delete'))` | |
| `plan_hash` | `TEXT NULL` | sha256 of the compiled DataFusion plan |
| `decision` | `TEXT NOT NULL CHECK (decision IN ('permit','deny'))` | |
| `outcome` | `TEXT NOT NULL CHECK (outcome IN ('ok','denied','over_budget','circuit_open','plan_error','exec_error'))` | |
| `latency_ms` | `INT NOT NULL` | |
| `est_bytes` | `BIGINT NULL` | |
| `actual_bytes` | `BIGINT NULL` | |
| `raw_intent` | `JSONB NOT NULL` | archived for replay — opaque |
| `ts` | `TIMESTAMPTZ NOT NULL DEFAULT now()` | partition key (monthly) |

Time-partitioned monthly. BRIN index on `ts`. Optional S3 archival of cold partitions.

### 4.2 `telemetry_events`

Per `architecture §10.1`. Unified stream, dimensioned. Written by the PG audit sink batched (1000 events / 100 ms).

| Column | Type | Notes |
|---|---|---|
| `id` | `UUID PRIMARY KEY` | UUIDv7 |
| `ts` | `TIMESTAMPTZ NOT NULL` | partition key |
| `trace_id` | `UUID NOT NULL` | |
| `tenant_id` | `UUID NOT NULL` | RLS |
| `agent_id` | `UUID NULL` | |
| `source_id` | `UUID NULL` | |
| `connector_kind` | `TEXT NULL CHECK (connector_kind IN (…))` | |
| `entity_id` | `UUID NULL` | |
| `action` | `TEXT NULL CHECK (action IN ('read','append','update','delete'))` | |
| `event_kind` | `TEXT NOT NULL CHECK (event_kind IN ('intent_received','intent_authorized','intent_routed','plan_compiled','plan_rejected','execution_started','execution_finished','artifact_emitted','source_health','source_drift','agent_rate_limited'))` | |
| `status` | `TEXT NULL CHECK (status IN ('ok','denied','over_budget','circuit_open','plan_error','exec_error'))` | |
| `latency_ms` | `INT NULL` | |
| `est_bytes`, `actual_bytes` | `BIGINT NULL` | |
| `tail` | `JSONB NULL` | narrow, connector-specific extras only |

### 4.3 `source_health`, `schema_snapshots`, `cost_records`, `proposals`

Detailed designs deferred to the phase in which they land (Phase 10 health, Phase 11 drift, Phase 9 cost, Phase 13 proposals). All follow the same conventions: UUIDv7 PK, SCD Type 2 if state-bearing, time-partitioned if high-volume.

---

## 5. Accounts & grants

Per `architecture §5.10`, `zero-trust §10`, `§13`.

### 5.1 Control-plane Postgres

| Role | Grants | Held by |
|---|---|---|
| `eh_admin` | All DDL; `GRANT` / `REVOKE`; full `SELECT` / `INSERT` / `UPDATE` / `DELETE` on every table in `eh_control` and `eh_operational`; migration authority | **Operator only.** Application never authenticates as this role. |
| `eh_service` | `SELECT` on every table in `eh_control`; `SELECT` + `INSERT` on every table in `eh_operational`; no `UPDATE`, no `DELETE`, no DDL — engine-refused | Application. Credentials injected from secrets manager. |

`eh_service` connections issue `SET LOCAL app.agent_id = $1` and `SET LOCAL app.tenant_id = $2` at the start of every request transaction; RLS policies on `eh_operational` tables filter on these.

### 5.2 Per-tenant data DBs (Phase 6+)

Same shape, parallel pair: `eh_admin_<tenant>` (operator) + `eh_service_<tenant>` (application). One physical database per tenant — no cross-tenant query path (`architecture §5.8`).

### 5.3 FVP MySQL (Phase 1)

Created by `examples/compose/mysql-init.sql`. Two users:

```sql
CREATE USER 'eh_admin'@'%' IDENTIFIED BY '<operator-only, env-injected>';
GRANT ALL PRIVILEGES ON eh_demo.* TO 'eh_admin'@'%';

CREATE USER 'eh_service'@'%' IDENTIFIED BY '<env-injected from secrets manager ref>';
GRANT SELECT ON eh_demo.customers TO 'eh_service'@'%';
-- explicitly NO INSERT, UPDATE, DELETE, DDL
```

When append/update arrives in later phases, the operator extends the grants per `§11`.

---

## 6. Phase 1 worked example — tenant `customers` table on MySQL

The data-source side of the FVP. Operator-applied via `examples/compose/mysql-init.sql`.

```sql
CREATE DATABASE eh_demo;
USE eh_demo;

CREATE TABLE customers (
  id            BINARY(16) NOT NULL PRIMARY KEY,           -- UUIDv7
  name          VARCHAR(255) NOT NULL,
  email         VARCHAR(255) NOT NULL,
  signup_at     TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
  ltv_usd       DECIMAL(12,2) NOT NULL DEFAULT 0.00,

  valid_from    TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
  valid_to      TIMESTAMP(6) NOT NULL DEFAULT '9999-12-31 23:59:59.999999',
  is_current    TINYINT(1)   NOT NULL DEFAULT 1,

  INDEX idx_email (email),
  INDEX idx_signup_at (signup_at),
  INDEX idx_chain (id, valid_from)
);

-- 10 seeded customers (UUIDv7 generated; insert-only)
INSERT INTO customers (id, name, email, signup_at, ltv_usd) VALUES
  (UUID_TO_BIN(UUID(), 1), 'Alice Example', 'alice@example.com', NOW(), 100.00),
  -- ... 9 more
;
```

> **MySQL `UUID()` is v1, not v7.** The seed script uses Rust-generated UUIDv7 strings pre-computed at init time, or shells out to a small `eh-seed` helper. The application code generating IDs at runtime always uses the `uuid7` Rust crate — never MySQL's `UUID()`.

---

## 7. Migrations workflow

Per `zero-trust §11` (Schema-First, Code-After):

1. **Design.** Model proposes a schema change; operator approves; SCHEMA.md is updated *first* and becomes the source of truth for the change.
2. **Operator applies.** The operator runs the migration under the `eh_admin` role. The application never does this.
3. **Code follows.** Application code updates services, callers, and tests against the now-locked schema using its `eh_service` role.

The migration toolchain is `sqlx migrate` (V1) — a Phase 6 deliverable for the control-plane DB. For Phase 1 (no control plane yet), the MySQL init script is the only migration; subsequent data-source schema changes follow the same workflow.

Authorization per change is **per-change, scoped**: approval for one change does not extend to adjacent or "while-I'm-here" changes (`zero-trust §11`).

---

## 8. Drift detection

Phase 11 deliverable. Compares this document (the design) against `information_schema` introspection (the live shape) per source. Emits `SourceDrift` telemetry events with `warn` / `fail_closed` / `shadow` severity per binding policy.

The detector **never auto-applies** entity-schema changes (`architecture §12`). It produces a diff that the operator reviews; in V2.0 the async copilot (Phase 13) drafts PRs against this document for the operator to approve.

---

---

## Appendix A — Table Index (fully-qualified)

The complete set of tables this document covers. The CI guardrail (`scripts/check-schema-sync.sh`) asserts that this set matches the set declared in `db/postgres/migrations/*.sql` and `db/mysql/init/*.sql`.

### `eh_control` (config)

- `eh_control.tenants`
- `eh_control.agents`
- `eh_control.agent_secrets`
- `eh_control.policies`
- `eh_control.capabilities`
- `eh_control.sources`
- `eh_control.source_credentials`
- `eh_control.source_mysql`
- `eh_control.source_postgres`
- `eh_control.source_iceberg`
- `eh_control.source_duckdb`
- `eh_control.source_rag`
- `eh_control.source_model`
- `eh_control.source_file`
- `eh_control.source_snowflake`
- `eh_control.source_mssql`
- `eh_control.source_duckdb_extensions`
- `eh_control.entities`
- `eh_control.entity_fields`
- `eh_control.entity_bindings`
- `eh_control.entity_binding_actions`
- `eh_control.entity_field_bindings`
- `eh_control.routing_rules`
- `eh_control.predicate_nodes`
- `eh_control.entity_relationships`

### `eh_operational` (events)

- `eh_operational.audit_log`
- `eh_operational.telemetry_events`
- `eh_operational.source_health`
- `eh_operational.schema_snapshots`
- `eh_operational.cost_records`
- `eh_operational.proposals`

### `eh_demo` (MySQL FVP data source)

- `eh_demo.customers`

---

## Appendix B — DDL Locations

The canonical SQL DDL lives in `db/`:

| Schema | DDL file(s) |
| --- | --- |
| `eh_demo.*` (MySQL FVP) | `db/mysql/init/01_fvp_schema.sql` |
| `eh_control` + `eh_operational` schemas + roles | `db/postgres/migrations/0001_schemas_and_roles.sql` |
| `eh_control.tenants`, `agents`, `agent_secrets` | `db/postgres/migrations/0002_identity.sql` |
| `eh_control.policies`, `capabilities` | `db/postgres/migrations/0003_authorization.sql` |
| `eh_control.sources`, `source_credentials` | `db/postgres/migrations/0004_sources_core.sql` |
| `eh_control.source_<kind>` family + `source_duckdb_extensions` | `db/postgres/migrations/0005_source_kinds.sql` |
| `eh_control.entities`, `entity_fields` (+ deferred FKs) | `db/postgres/migrations/0006_semantic_schema.sql` |
| `eh_control.entity_bindings`, `entity_binding_actions`, `entity_field_bindings`, `routing_rules`, `predicate_nodes` | `db/postgres/migrations/0007_bindings_routing.sql` |
| `eh_control.entity_relationships` | `db/postgres/migrations/0008_entity_relationships.sql` |
| `eh_operational.audit_log` (partitioned) | `db/postgres/migrations/0009_operational_audit.sql` |
| `eh_operational.telemetry_events`, `source_health`, `schema_snapshots`, `cost_records`, `proposals` | `db/postgres/migrations/0010_operational_other.sql` |
| RLS policies on every tenant-scoped table | `db/postgres/migrations/0011_rls_policies.sql` |

---

*Document version: v0.1 (DRAFT) · maintained at [github.com/k8nstantin/eventhorizon](https://github.com/k8nstantin/eventhorizon) · subject to per-table operator approval per `zero-trust §11`.*
