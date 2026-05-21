# Building an EventHorizon Connector

EventHorizon's connector surface is **public, stable, and intentionally open**. Anyone can publish a connector for any backend that returns tabular data — Postgres, MySQL, Iceberg, DuckDB, Snowflake, SQL Server, RAG/vector stores, LLM endpoints, Parquet directories, Neo4j, MongoDB, your proprietary system. **MySQL is one of many; nothing is hardcoded into the kernel.**

This guide is the practical path from "I have a backend" to "EventHorizon routes intents to it." See also: [architecture §4 / §9](./eventhorizon_architecture.md#4-data-plane--datafusion-backed-federation) and [SCHEMA.md §3.7](./SCHEMA.md#37-source_kind-per-connector-tables-the-proxysql-pattern).

---

## The contract

A connector implements two things:

1. **The `Connector` Rust trait** (`crates/eh-connector-api`) — the runtime contract. Returns a DataFusion `TableProvider`; DataFusion handles plan optimization, predicate pushdown, projection pushdown, streaming.
2. **A `source_<kind>` config table** in `eh_control` — the typed schema for your connector's configuration. Operator-approved migration; no `JSONB` metadata catch-alls.

Plus your connector lives in its own crate (e.g. `eh-connector-neo4j`), depends only on `eh-connector-api`, and is registered in `eh-bin` via a Cargo feature.

**Zero kernel changes are required** to add a new connector kind. The kernel is closed for modification, open for extension.

---

## The five-step workflow

### 1. Schema-first: propose your `source_<kind>` table

Per [zero-trust §11](./.claude/skills/zero-trust-execution/SKILL.md) (Schema-First, Code-After), every connector starts with a schema design PR.

Open a PR adding two files:

- A new migration: `db/postgres/migrations/NNNN_source_<your_kind>.sql`
- An update to [`SCHEMA.md`](./SCHEMA.md) (Appendix A table index + a section in §3.7 describing your config columns)

Your migration follows the established pattern. Every connector config table:

- Has `source_id UUID PRIMARY KEY REFERENCES eh_control.sources(id)` (1:1 with the kind-agnostic `sources` row).
- Has the SCD Type 2 triad (`valid_from`, `valid_to`, `is_current`).
- Uses **typed columns** (no `JSONB` for structured config).
- Uses `CHECK` enums for any status / mode / auth_kind columns.
- References secrets via `*_secret_ref` columns (`vault://…`, `k8s://…`, `aws-sm://…`, `gcp-sm://…`, `file://…`, `env://…`). **Never store secret values.**

Also extend the `sources.kind` `CHECK` enum to include your new kind:

```sql
ALTER TABLE eh_control.sources
  DROP CONSTRAINT sources_kind_chk,
  ADD CONSTRAINT sources_kind_chk
    CHECK (kind IN ('mysql','postgres','iceberg','duckdb','rag','model','file','snowflake','mssql','<your_kind>'));
```

Add RLS policy in a follow-up migration (or the same one):

```sql
ALTER TABLE eh_control.source_<your_kind> ENABLE ROW LEVEL SECURITY;
ALTER TABLE eh_control.source_<your_kind> FORCE ROW LEVEL SECURITY;

CREATE POLICY source_<your_kind>_select_tenant ON eh_control.source_<your_kind>
  FOR SELECT TO eh_service
  USING (
    EXISTS (
      SELECT 1 FROM eh_control.sources s
      WHERE s.id = source_<your_kind>.source_id
        AND s.tenant_id = eh_control.current_tenant_id()
    )
  );
```

The CI guardrail `scripts/check-schema-sync.sh` asserts your table appears in both `SCHEMA.md` and the DDL.

### 2. Implement the `Connector` trait

Once the schema PR is approved and merged, open your connector crate. It depends only on `eh-connector-api`:

```toml
[package]
name = "eh-connector-neo4j"
description = "EventHorizon connector for Neo4j"
version = "0.1.0"
edition = "2021"
license = "Apache-2.0"

[dependencies]
eh-connector-api = "0.1"        # the stable public trait
async-trait = "0.1"
# … your Neo4j client crate, e.g. neo4rs
```

Implement `Connector`. Your `bind_entity` returns a DataFusion `TableProvider` — that's where the work happens. See [architecture §9 — the trait](./eventhorizon_architecture.md#the-trait-eh-connector-api) for the full signature.

The key methods:

| Method | What you implement |
| --- | --- |
| `kind()` | Returns the stable string identifier, e.g. `"neo4j"`. Must match `sources.kind`. |
| `capabilities()` | Honestly declare what you support: read, append, update, predicate / projection / aggregation pushdown, streaming, identity passthrough. Lies become bugs (the conformance suite catches them). |
| `connect(cfg)` | Open a pool. Read auth from `cfg`. Never embed credentials. |
| `health()` | Cheap liveness ping. |
| `bind_entity(binding)` | Return a `TableProvider` for the bound entity. DataFusion does the rest. |
| `describe(scope)` | Introspect physical schema. Drives drift detection (Phase 11). |
| `write(intent, ctx)` | Optional. Default `Err(WriteError::Unsupported)`. Implement only if you opted-in to append/update in `capabilities()`. |
| `prepare_session(ctx)` | Optional. For backends that need per-request identity passthrough (e.g. `SET LOCAL` style). |

### 3. Pass the conformance suite

Every connector must pass `eh-conformance` to earn the **verified** badge in the registry.

```bash
cargo test -p eh-conformance -- --connector eh-connector-neo4j --backend-uri $NEO4J_URI
```

The suite covers eight categories (see [architecture Appendix C](./eventhorizon_architecture.md#appendix-c--connector-conformance-suite)):

1. **Capability honesty** — declared `ConnectorCaps` matches behavior; pushed-down predicates produce identical results to non-pushed-down plans (property-based).
2. **Schema introspection** — `describe()` is stable and complete.
3. **CRUD semantics** — per declared support, basic flows work.
4. **Identity passthrough** — when declared, downstream session is correctly scoped.
5. **Cancellation** — async cancellation propagates within 100 ms.
6. **Resource discipline** — no FD or memory leak across 10k iterations.
7. **Error mapping** — backend errors map to the defined `ConnectError` taxonomy.
8. **Streaming** — when declared, large result sets stream without OOM.

A green run produces a signed conformance report.

### 4. Register in the runtime

In `crates/eh-bin/Cargo.toml`, add your connector as an optional dependency behind a feature flag:

```toml
[dependencies]
eh-connector-neo4j = { version = "0.1", optional = true }

[features]
connector-neo4j = ["dep:eh-connector-neo4j"]
all-connectors = [..., "connector-neo4j"]
```

In the wiring code, register it when the feature is on. Operators opt into your connector at build time via:

```bash
cargo build --release --features connector-neo4j
```

For first-party / community connectors that meet the bar, we'll add the feature to `default`.

### 5. List in the community registry

Open a PR to the `awesome-eventhorizon-connectors` README (in the org) listing:

- Crate name and link.
- Backend kinds supported (Neo4j Community / Enterprise / Aura).
- Declared capability surface (read / append / update / pushdowns).
- Conformance status (verified ✅ / pending / opt-out).
- Maintainer contact.

---

## Beyond SQL — non-database connectors

The `Connector` trait is broader than "SQL backend." Anything returning rows is a valid connector. Three live patterns:

| Kind | `bind_entity` returns a `TableProvider` whose scan yields… | Example intent |
| --- | --- | --- |
| **RAG** (`eh-connector-rag`) | Top-k passages from a vector store: `(doc_id, passage_text, score, metadata_kind)` | `{action: read, entity: KnowledgeBase, mode: similarity, query: "…", k: 8}` |
| **Model** (`eh-connector-model`) | One row per inference: `(prompt, response, tokens_in, tokens_out, latency_ms, model_id)` | `{action: read, entity: ChatCompletion, prompt: "…", model: "claude-opus-4-7"}` |
| **Files** (`eh-connector-files`) | Rows over Parquet/CSV/JSON files | `{action: read, entity: ServerLog, filter: {host: "edge-3"}}` |

The kernel doesn't know or care. The trait is the contract.

---

## What's already published (V1.0 first-party)

| Kind | Crate | Status | Notes |
| --- | --- | --- | --- |
| MySQL | `eh-connector-mysql` | first-party, Phase 1 FVP | UUIDv7 BINARY(16) native bind; password / mTLS / IAM (`auth_kind`) |
| Postgres | `eh-connector-postgres` | first-party, Phase 4 | password / mTLS / IAM; identity passthrough via `SET LOCAL` |
| Iceberg | `eh-connector-iceberg` | first-party, Phase 7 | `iceberg-rust`; partition pruning + manifest pushdown |
| DuckDB | `eh-connector-duckdb` | first-party | in-memory or file-backed; per-extension table for loadable extensions |
| Snowflake | `eh-connector-snowflake` | first-party, Phase 12 V1.1 | ADBC + Arrow native; `key_pair / password / oauth` |
| SQL Server | `eh-connector-mssql` | first-party, Phase 12 V1.1 | `tiberius`; `sql / integrated / aad` |
| RAG | `eh-connector-rag` | forward-looking | vector_store_uri + embedding_model |
| Model | `eh-connector-model` | forward-looking | anthropic / openai / mistral / local |
| File | `eh-connector-file` | forward-looking | Parquet/CSV/JSON over S3/GCS/local |

Community connectors land in `awesome-eventhorizon-connectors`. To get listed: file a PR there with the registry entry plus a link to your green conformance report.

---

## Security & governance for connector authors

By contributing a connector you are running inside the gateway process. Three rules:

1. **The application authenticates as `eh_service` (`SELECT` on `eh_control`, `SELECT + INSERT` on `eh_operational`, no UPDATE/DELETE/DDL).** Your connector code MUST NOT escape this — for example, by opening admin connections, switching roles mid-session, or implementing local UPDATE-style workarounds. Engine refusals are the [§12 debugging surface](./.claude/skills/zero-trust-execution/SKILL.md), not obstacles.
2. **Honesty in `capabilities()`.** Declaring `predicate_pushdown: Exact` and then pushing down a filter that changes the result set is a correctness bug the conformance suite is designed to catch. If unsure, declare `Inexact` or `None` — the planner adapts.
3. **No secret values in source.** Configuration goes through `source_<kind>` typed columns; secrets are `*_secret_ref` strings (`vault://…`, `k8s://…`, `aws-sm://…`, `gcp-sm://…`, `file://…`, `env://…`) resolved at runtime by the secrets-manager layer.

---

## Questions or proposals?

Open an issue with the `connector-proposal` label. For backends with non-tabular semantics (graph traversals, document search, time-series), describe your intended `TableProvider` shape — happy to iterate before you invest the implementation effort.
