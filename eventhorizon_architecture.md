# EventHorizon — Architecture v0.1

> **A modular, Rust-native semantic gateway between agentic (and human) consumers and a heterogeneous fleet of data sources.**

EventHorizon exposes a single typed surface (MCP/REST/gRPC), routes intents declaratively to the best-fit backend, compiles them through Apache DataFusion with cost-bounded planning, returns token-dense artifacts, emits fine-grained telemetry to OTel/Kafka, and is fully manageable via CLI, Terraform, and a React console — all backed by the same admin API.

This document is the load-bearing reference. As the design granularizes, each section will spawn a dedicated sub-doc and be linked from here.

---

## Table of Contents

1. [Problem & Positioning](#1-problem--positioning)
2. [Architecture at a Glance](#2-architecture-at-a-glance)
3. [The Kernel & Module System](#3-the-kernel--module-system)
4. [Data Plane — DataFusion-backed federation](#4-data-plane--datafusion-backed-federation)
5. [Control Plane — Postgres](#5-control-plane--postgres)
6. [Edge Protocols — MCP, REST, gRPC](#6-edge-protocols--mcp-rest-grpc)
7. [Routing — Declarative](#7-routing--declarative)
8. [Authorization (Cedar) & Identity Passthrough](#8-authorization-cedar--identity-passthrough)
9. [Connector Trait & Lifecycle](#9-connector-trait--lifecycle)
10. [Telemetry, Observability, Kafka Emission](#10-telemetry-observability-kafka-emission)
11. [Cost & Usage Governance](#11-cost--usage-governance)
12. [Schema Management — Two Schemas, Two Strategies](#12-schema-management--two-schemas-two-strategies)
13. [Operator Surface — CLI, UI, Terraform](#13-operator-surface--cli-ui-terraform)
14. [Kubernetes Deployment](#14-kubernetes-deployment)
15. [Security Model](#15-security-model)
16. [Cross-Cutting Principles](#16-cross-cutting-principles)
17. [V1 Scope & Non-Goals](#17-v1-scope--non-goals)
18. [Roadmap](#18-roadmap)
19. [Glossary](#19-glossary)
20. [Phased Implementation Plan](#20-phased-implementation-plan)
21. [Appendix A — Crate Layout](#appendix-a--crate-layout)
22. [Appendix B — Worked Example: Federated Customer Entity](#appendix-b--worked-example-federated-customer-entity)
23. [Appendix C — Connector Conformance Suite](#appendix-c--connector-conformance-suite)

---

## 1. Problem & Positioning

### The problem

Giving an LLM agent direct database access (SQL, ORM, or a thinly-typed REST API with a visible schema) is a known antipattern:

- **Schema reconnaissance.** Agents probe the edges of the schema and surface PII or hidden tables through trial-and-error.
- **Hallucinated SQL.** Agents invent columns, dialects, or join paths, producing wrong-but-plausible answers.
- **Destructive escapes.** Prompt injection or hallucination leads to `DROP`, `DELETE`, `ALTER` issued through legitimate connections.
- **Token waste.** Raw rowsets, metadata, and verbose schemas blow up LLM context.
- **Cost blowouts.** Analytical sources (BigQuery, Snowflake, Iceberg-on-object-storage) bill per byte scanned. A poorly-formed agent query is a $-shaped event.
- **Operational fragility.** Schema migrations behind the agent's back silently break its worldview.

### The positioning

EventHorizon is the gateway that sits between **agentic (and human) consumers** and a **heterogeneous fleet of data sources**, providing:

- A typed semantic surface (MCP-native, REST/gRPC alongside).
- Declarative routing across polyglot backends (OLTP, lakehouse, warehouse).
- Cost-bounded query planning via Apache DataFusion.
- Token-dense artifact emission (projection + aggregation pushdown).
- Cedar-based authorization with backend identity passthrough.
- Fine-grained, typed telemetry — emitted to OTel and Kafka.
- A connector model that lets the community add Snowflake, SQL Server, Neo4j, or anything else.
- A single admin API backing CLI, React UI, and Terraform.

### The landscape

| Tool | Federates? | Typed agent contract? | Token-aware artifacts? | MCP-native? |
| --- | --- | --- | --- | --- |
| Trino / Presto | yes | no — raw SQL | no | no |
| Hasura / PostgREST | no (single backend) | yes | partial | no |
| DataFusion / Arrow Flight | yes (Rust) | no — SQL/DataFrames | no | no |
| Cube.js | partial (BI cubes) | yes | partial | no |
| LangChain SQL agent | per-tool | no — text-to-SQL | no | sort-of |
| MCP servers (per-DB) | no | yes | no | yes |
| **EventHorizon** | **yes** | **yes** | **yes** | **yes** |

The four-corner cell is the moat.

---

## 2. Architecture at a Glance

```
                        ┌────────────────────────────────────┐
   Agents (Claude,       │     EventHorizon Pod (Rust)        │
   Cursor, swarms) ────► │  ┌──────┐  ┌──────────────────┐    │
                        │  │ MCP  │  │  Intent →         │    │
   Human apps    ─────► │  │ REST │─►│  Plan (DataFusion)│────┼──► Postgres OLTP (Connector)
                        │  │ gRPC │  │                  │    │
                        │  └──────┘  └─────────┬────────┘    │──► Iceberg lake   (Connector)
                        │              │       │             │
                        │      ┌───────▼───┐   │             │──► Snowflake     (Community)
                        │      │ Authz     │   │             │
                        │      │ (Cedar)   │   ▼             │──► SQL Server    (Community)
                        │      └───────────┘  Artifact       │
                        │      ┌───────────┐  Compiler       │──► … any TableProvider
                        │      │ Router    │   │             │
                        │      │ (in-mem)  │   ▼             │
                        │      └───▲───────┘  JSON / Arrow   │
                        │          │  ArcSwap reload         │
                        │   ┌──────┴──────┐                  │
                        │   │ Schema/Caps │                  │
                        │   │ cache       │                  │
                        │   └──────▲──────┘                  │
                        │   ┌──────┼──────┐                  │
                        │   │ Telemetry   │── OTel ──► Tempo/Jaeger
                        │   │ Event Bus   │── Prom ──► Grafana
                        │   │             │── Kafka ─► downstream
                        │   └──────┬──────┘── Audit ─► Postgres / S3
                        └──────────┼──────────────────────────┘
                                   │ LISTEN/NOTIFY + 60s reload
                            ┌──────┴──────────────┐
                            │ Postgres            │
                            │ (control plane:     │
                            │  agents, entities,  │
                            │  bindings, rules,   │
                            │  audit log)         │
                            └─────────────────────┘
                                   ▲
                                   │ admin API (REST + OpenAPI)
                       ┌───────────┼───────────┐
                       │           │           │
                  ┌────┴───┐ ┌─────┴────┐ ┌────┴─────┐
                  │ eh ctl │ │  React   │ │ Terraform│
                  │  CLI   │ │ Console  │ │ Provider │
                  └────────┘ └──────────┘ └──────────┘
                                   ▲
                            ┌──────┴─────────────┐
                            │ Async Copilot      │
                            │ (Gemma, optional)  │
                            │  reads telemetry,  │
                            │  drafts MV/route   │
                            │  recommendations   │
                            └────────────────────┘
```

---

## 3. The Kernel & Module System

The kernel is small, sharp, and performs no I/O. Everything else is a module that plugs in via a trait.

### Cargo workspace

```
eventhorizon/
├── crates/
│   ├── eh-core/              # Intent, Plan, Artifact, Entity, Binding types. No I/O.
│   ├── eh-protocol/          # canonical JSON schemas, OpenAPI, MCP tool descriptors
│   ├── eh-connector-api/     # the Connector trait + DataFusion glue + ConnectorCaps
│   ├── eh-control-api/       # ControlPlane trait — agents, entities, bindings, rules
│   ├── eh-policy/            # Cedar wrapper; pure-function authz decisions
│   ├── eh-router/            # declarative rule evaluator, ArcSwap-backed cache
│   ├── eh-compiler/          # Intent → DataFusion LogicalPlan; Artifact compaction
│   ├── eh-telemetry/         # typed event bus, Sink trait, OTel integration
│   ├── eh-cost/              # plan-cost estimator, budgets, quotas
│   │
│   ├── eh-edge-mcp/          # MCP server module
│   ├── eh-edge-rest/         # REST server module
│   ├── eh-edge-grpc/         # gRPC server module (optional)
│   │
│   ├── eh-control-pg/        # default Postgres ControlPlane impl
│   ├── eh-sink-kafka/        # Kafka telemetry sink
│   ├── eh-sink-otlp/         # OTel/OTLP telemetry sink
│   ├── eh-sink-s3-audit/     # immutable audit log archive
│   │
│   ├── eh-connector-postgres/
│   ├── eh-connector-mysql/
│   ├── eh-connector-iceberg/
│   ├── eh-connector-duckdb/
│   │
│   ├── eh-conformance/       # connector test-kit (property + integration)
│   ├── eh-bin/               # the binary: wires selected modules per build profile
│   └── eh-ctl/               # the CLI
│
├── ui/                       # React/Vite admin console
├── terraform-provider-eventhorizon/   # Go, lives in its own repo when published
└── examples/
    ├── helm/
    ├── compose/
    └── dashboards/
```

### Rules of the kernel

- `eh-core` depends only on `serde` and `arrow`. It is the lingua franca.
- Connectors depend only on `eh-connector-api` (which re-exports DataFusion's `TableProvider`).
- Edges depend on `eh-core` + `eh-protocol`. They never touch a connector directly — they emit `Intent` into a `Pipeline` provided by the kernel.
- Sinks depend only on `eh-telemetry`. A Kafka outage cannot break the request path.
- `eh-bin` is the only crate that imports everything. Custom builds (e.g., a customer's proprietary Vertica connector) replace `eh-bin` with their own thin wiring crate.

### Cargo features for slim builds

```toml
[features]
default = ["edge-mcp", "edge-rest", "connector-postgres", "connector-iceberg", "sink-otlp"]
all-connectors = ["connector-postgres", "connector-mysql", "connector-iceberg", "connector-duckdb"]
slim = ["edge-mcp", "connector-postgres"]
```

### The meta-insight: the control plane is itself a logical data source

The control plane is addressable through the same `Connector` abstraction it controls. The admin REST surface queries `eh-control-pg` through the same router and compiler, with a **separate `control:*` capability namespace**. Benefits:

- Uniform observability and audit for admin operations.
- Easy testing with mock control planes.
- Future-proof for federated/sharded control planes.

---

## 4. Data Plane — DataFusion-backed federation

EventHorizon does **not** hand-roll dialect translation. Apache DataFusion is the data plane.

### Pipeline

```
Intent (typed JSON)
   │
   ▼
Validation        — eh-protocol type-checks against the entity schema
   │
   ▼
Authorization    — eh-policy evaluates against cached Cedar policies
   │
   ▼
Routing          — eh-router picks (source, binding, rule) by declarative rules
   │
   ▼
Plan Compilation — eh-compiler builds a DataFusion LogicalPlan against the
                    selected TableProvider(s)
   │
   ▼
Cost Gating      — eh-cost rejects plans over budget / quota / circuit-open
   │
   ▼
Execution        — DataFusion executes; predicate, projection, and aggregation
                    pushdowns are honored per ConnectorCaps
   │
   ▼
Artifact         — RecordBatch stream → projection-pruned, type-tight JSON
                    (or Arrow IPC for power callers)
   │
   ▼
Emission         — telemetry events emitted at every stage
```

### Why DataFusion

- Rust-native, Apache-licensed.
- Mature query planner and optimizer.
- Pluggable `TableProvider` model — connectors return one and DataFusion handles the rest.
- Free predicate / projection / aggregation pushdown when connectors advertise it.
- Streaming `RecordBatch` execution; we never materialize whole result sets.
- First-class Arrow throughout, which aligns with the **ADBC** ecosystem we're betting on long-term.

### Artifact compaction (the token saver)

In order of value:

1. **Projection compilation** — only the requested fields are scanned, returned, and serialized. ~80% of token savings come from this alone.
2. **Aggregation pushdown** — `mode: trend` with `group_by: month` emits a single `GROUP BY` query, returning O(buckets) rows instead of O(events).
3. **Materialized / cached artifacts** — plan-hash-keyed plan cache; intent-hash + source-watermark artifact cache (V2).
4. **Optional summarization** — `format: narrative` flag opts in to LLM-based summarization on top. Off by default.

---

## 5. Control Plane — Postgres

The control plane is **boring on purpose**: Postgres, transactions, replication, the well-trodden path.

### What it holds

- `agents` — id, name, token_hash, status, cost_budget, metadata
- `entities` — id, name, version, description, json_schema
- `sources` — id, kind, config (jsonb, secret-refs only), ring (staging/production), status
- `entity_bindings` — entity_id, source_id, table_ref, field_map (jsonb), supported_actions, profile
- `routing_rules` — entity_id, predicate (jsonb), target_source_id, priority, version
- `capabilities` — agent_id, entity_id, allowed_actions, conditions (jsonb)
- `audit_log` — append-only intent history with full trace_id, plan_hash, outcome
- `schema_snapshots` — physical schema crawled from each source, time-versioned
- `materialized_recommendations` — output of the async copilot, awaiting human review

### Hot-path strategy

- Every pod holds an in-process `Arc<ArcSwap<CompiledConfig>>` containing:
  - routing rules (compiled to fast lookup form)
  - capability map per agent
  - entity schemas
  - source registry with health/circuit state
- Refresh is push-based via Postgres `LISTEN/NOTIFY`, with a 60-second periodic full-reload safety net.
- Hot-path lookup is nanoseconds; no network call per intent.

### Why not SurrealDB / a graph DB?

The data is not graph-shaped. The hot-path query is a single indexed point lookup or 2-table join. Postgres + `ArcSwap` cache wins on durability, ops familiarity, managed-cloud availability, and replication maturity. If multi-hop authorization ever becomes needed, the right answer is **OpenFGA / SpiceDB** (Zanzibar-style), not a generalist multi-model DB.

---

## 6. Edge Protocols — MCP, REST, gRPC

Three edges, one kernel.

### MCP (primary)

- Each entity is published as a typed MCP tool (e.g., `customer.read`, `customer.append`).
- Discovery is automatic; Claude/Cursor/agents see the surface immediately.
- Tool descriptors include allowed actions, field schemas, and example intents.
- This is the wedge — adoption follows MCP-native distribution.

### REST

- `POST /v1/intent` — single semantic intent.
- `POST /v1/intent/batch` — bulk.
- `GET /v1/entities` — discover the surface.
- `POST /admin/v1/...` — admin operations (separate auth tier).
- OpenAPI 3.1 generated from Rust types via `utoipa`.

### gRPC (optional)

- For high-throughput swarm-to-gateway internal traffic.
- Same protocol semantics as REST.
- Compiled feature; off by default.

### Stateless

All edges are stateless. A pod can be terminated mid-request; the client retries; the next pod handles it. No sticky sessions.

---

## 7. Routing — Declarative

Routing decisions belong in **code and config, not in an LLM**. The router is a deterministic function.

### Why not LLM-routed?

- Latency floor — even Gemma-2B adds 50–300 ms to every decision.
- Non-determinism — routing must be replayable and auditable.
- Cost — burning CPU/GPU per query is unjustifiable when a hash lookup suffices.
- Debuggability — "the LLM chose" is not a trace.
- Security — payload content could influence routing decisions (prompt-injection surface).

### How routing actually works

A YAML rule file (versioned in git, applied via admin API) declares routes per entity:

```yaml
entities:
  Customer:
    sources:
      pg_oltp:
        kind: postgres
        table: customers
        supports: [read, append, update]
      ice_history:
        kind: iceberg
        table: warehouse.customers_hist
        supports: [read]
        profile: analytical
    routing:
      - when: { action: append }                                  → pg_oltp
      - when: { action: read, mode: point }                       → pg_oltp
      - when: { action: read, mode: [trend, aggregate, window] }  → ice_history
      - when: { action: read, window: ">=30d" }                   → ice_history
      - default                                                    → pg_oltp
```

The router compiles this once into a fast in-memory decision tree. Hot-path evaluation is sub-microsecond.

### Where Gemma actually fits

Gemma (or any LLM) runs **off the hot path** as an **operator copilot**:

- Reads the telemetry stream asynchronously.
- Suggests new routing rules, materialized views, schema mappings, anomaly investigations.
- Drafts PRs that humans review and merge.

Never in the request path. Never trusted to act unilaterally.

---

## 8. Authorization (Cedar) & Identity Passthrough

### Authorization

- Policies live in **Cedar** (Rust-native, declarative, auditable).
- Policies are loaded into the per-pod cache and evaluated as pure functions on the hot path.
- Decisions are emitted to telemetry with the matched policy version.

Example policy:

```cedar
permit (
  principal in Agent::"role:analyst",
  action in [Action::"read"],
  resource in Entity::"Customer"
) when {
  resource.profile == "analytical" &&
  context.window <= duration("90d")
};
```

### Identity passthrough

The agent never connects to the backend. The gateway:

1. Establishes a pooled backend connection under a service account.
2. Per request, sets the caller identity on the session — for Postgres: `SET LOCAL app.agent_id = $1` inside a transaction.
3. Backend RLS / row-policy evaluates on `current_setting('app.agent_id')`.

This gives row-level isolation **at the backend**, not in the proxy, which is the only place it's safe.

---

## 9. Connector Trait & Lifecycle

Connectors are first-class. The community adds Snowflake, SQL Server, Neo4j, etc., as standalone crates.

### The trait (`eh-connector-api`)

```rust
#[async_trait]
pub trait Connector: Send + Sync + 'static {
    fn kind(&self) -> &'static str;             // "postgres", "iceberg", "snowflake"
    fn capabilities(&self) -> ConnectorCaps;

    async fn connect(cfg: ConnectorConfig) -> Result<Arc<Self>, ConnectError>
    where Self: Sized;

    async fn health(&self) -> Health;

    /// Bind a logical entity to a DataFusion TableProvider.
    async fn bind_entity(
        &self,
        binding: &EntityBinding,
    ) -> Result<Arc<dyn TableProvider>, BindError>;

    /// Introspect physical schema for drift detection.
    async fn describe(&self, scope: DescribeScope) -> Result<PhysicalSchema, DescribeError>;

    /// Optional write path; default returns Unsupported.
    async fn write(
        &self,
        intent: &WriteIntent,
        ctx: &CallerContext,
    ) -> Result<WriteOutcome, WriteError> {
        Err(WriteError::Unsupported)
    }

    /// Optional session prep (e.g., SET LOCAL app.agent_id for PG RLS).
    async fn prepare_session(&self, _ctx: &CallerContext) -> Result<(), SessionError> {
        Ok(())
    }
}

pub struct ConnectorCaps {
    pub supports_read: bool,
    pub supports_append: bool,
    pub supports_update: bool,
    pub predicate_pushdown: PushdownLevel,   // None | Inexact | Exact
    pub projection_pushdown: bool,
    pub aggregation_pushdown: bool,
    pub join_pushdown: bool,
    pub streaming: bool,
    pub identity_passthrough: bool,
}
```

### The five-stage onboarding lifecycle

Every connector goes through the same lifecycle, gated and audited:

1. **Register** — `eh ctl source add` validates config, stores in control PG (secret-refs only), runs initial health probe. Status: `registered`.
2. **Probe** — `eh ctl source probe` calls `describe()`, caches `physical_schema_snapshot`. UI renders a navigable tree. Status: `probed`.
3. **Bind** — `eh ctl entity bind` (or UI drag-and-drop) maps logical fields to physical columns. A `LIMIT 3` validation query confirms the plan. Status: `bound`.
4. **Shadow / dry-run** — `eh ctl intent test --explain` previews plan + cost; `shadow_traffic_percent: 5` silently dual-executes against the new binding for telemetry comparison. Status: `staging`.
5. **Promote** — `eh ctl source promote --ring production` plus a routing rule add. NOTIFY fires; pods swap their `ArcSwap` within a second. Status: `production`.

Each stage is reversible; rollback is supported at every step.

### Distribution

- `eh-connector-api` published to crates.io, semver-stable.
- First-party connectors: `eh-connector-postgres`, `eh-connector-mysql`, `eh-connector-iceberg`, `eh-connector-duckdb`.
- Community-maintained list: `awesome-eventhorizon-connectors` in the org.
- **Conformance suite**: `eh-conformance` crate. Every connector must pass property-based pushdown-correctness, schema-introspection, and CRUD tests. Verified badge on green CI.
- Starter template: `cargo generate eventhorizon/connector-template`.
- **Snowflake / SQL Server** target ADBC + Arrow as the long-term substrate.
- WASM / dynamic loading: **deferred** to V3 unless community demand emerges. Compile-time plugins are the V1 model.

---

## 10. Telemetry, Observability, Kafka Emission

"You can't improve what you can't see." Telemetry is the spine, not an afterthought.

### Typed event stream

Every intent walks a fixed lifecycle, emitting structured events:

```rust
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TelemetryEvent {
    IntentReceived    { ts, trace_id, agent_id, raw: Intent },
    IntentAuthorized  { ts, trace_id, decision: Decision, policy_version: String },
    IntentRouted      { ts, trace_id, source_id, rule_id, reason: String },
    PlanCompiled      { ts, trace_id, plan_hash, est_rows, est_bytes, est_cost_cents },
    PlanRejected      { ts, trace_id, reason: RejectReason },
    ExecutionStarted  { ts, trace_id, source_id },
    ExecutionFinished { ts, trace_id, source_id, actual_rows, actual_bytes, latency_ms, status },
    ArtifactEmitted   { ts, trace_id, bytes_out, fields_pruned, cache_hit: bool },
    SourceHealth      { ts, source_id, state: HealthState, latency_ms },
    SourceDrift       { ts, source_id, entity_id, diff: SchemaDiff, severity },
    AgentRateLimited  { ts, agent_id, bucket, retry_after_ms },
}
```

### The bus and the sinks

- In-process `tokio::sync::broadcast` channel, bounded.
- One `Sink` trait, many implementations: `OtlpSink`, `KafkaSink`, `S3AuditSink`, `PgAuditSink`, `PromMetricsSink`.
- Each sink runs in its own task with its own bounded buffer.
- A failing sink **never blocks the request path** — backpressure drops oldest events and increments `eh_telemetry_drops_total{sink=…}`.

### Kafka emission (fire-and-forget, schema'd)

- `rdkafka` async producer with `acks=1`.
- Topic strategy: one topic per event family with versioned suffix — `eh.intent.v1`, `eh.health.v1`, `eh.drift.v1`.
- Serialization: Avro or Protobuf, Confluent Schema Registry compatible.
- Batched by count or time (1000 events / 100 ms).
- Downstream consolidation (lakehouse landing, billing, anomaly detection) is not the gateway's concern. **You are a producer.**

### Audit log — different contract

Telemetry is best-effort; **audit is durable**. Per-intent audit records are written transactionally to control-plane Postgres. If audit write fails, the request fails. Optionally archived to S3 (append-only, immutable) for compliance.

---

## 11. Cost & Usage Governance

Promote "don't hammer BigQuery" to a named subsystem: **`eh-cost`**.

### Layer A — Pre-execution gating (synchronous)

Before execution, the compiler asks DataFusion for plan statistics. `eh-cost` checks:

```
est_bytes_scanned     <= source.per_query_byte_limit ?
agent.cost_units_used <= agent.cost_budget_per_hour ?
source.in_flight      <= source.max_concurrent ?
source.circuit_state  == Closed ?
```

Any failure → `IntentRejected { reason: OverBudget | RateLimited | CircuitOpen }`. The agent receives a structured error with `retry_after_ms`. Never a 500.

### Layer B — Adaptive quotas (asynchronous)

A background task reads the telemetry stream and adjusts:

- Token-bucket replenishment per agent.
- Source weight degrades when p99 latency degrades.
- Rolling per-(agent, entity, action) cost average; expensive callers throttled first under pressure.

### Layer C — Cost-aware caching & materialization

The async copilot watches telemetry and emits **proposals** (PR-shaped recommendations, never auto-applied):

- "Agent `analyst_bot` ran `Customer trend 90d` 4012 times in 24h — promote to a daily materialized view, est. saving $X/day."
- "Source `bigquery_prod` p99 latency rising — consider routing `analytical` profile to `iceberg_prod`."
- "Intent shape `Order aggregate by store_id mode=ytd` would benefit from a DuckDB cache layer."

### Shipped dashboards (Grafana JSON)

- Cost per agent per day
- Cost per entity per day
- Cost per source per day, with budget overlay
- Top-N expensive intent shapes
- Cache hit-rate
- Drift events per source
- Circuit-breaker state per source

---

## 12. Schema Management — Two Schemas, Two Strategies

There are two schemas. Conflating them is the root cause of every messy gateway.

### (a) Entity Schema — logical, agent-facing, *human-authored*

- Source of truth: YAML/RON files in git, applied through CI or admin API.
- Runtime storage: rows in control-plane PG, written through an admin API that validates against the DSL.
- Hot-path refresh: `LISTEN/NOTIFY` pushes to all pods; each pod swaps `Arc<ArcSwap<Schema>>` atomically (sub-second).
- Safety net: 60-second periodic full reload.
- **Never auto-mutated.** Only humans (or CI) change the entity schema.

### (b) Physical Schema — what backends actually have, *discovered*

A background drift detector per source, configurable cadence:

- OLTP (PG / MySQL / MSSQL): every 30–60 s
- Iceberg / lakehouse: every 5–15 min
- Snowflake / BigQuery: every 5 min (metered `INFORMATION_SCHEMA` — don't hammer)

Workflow:

1. Each task calls the connector's `describe(scope)`.
2. Diff against the declared `entity_binding`.
3. Emit `SourceDrift` telemetry event with severity.
4. Apply per-binding policy: `warn` (default) | `fail_closed` | `shadow`.

Drift **never auto-applies** entity schema changes. It produces a diff that ops review (and the async copilot may draft a PR for).

---

## 13. Operator Surface — CLI, UI, Terraform

### Contract: one API, three faces

- **Admin REST API at `/admin/v1/*` is the only write path** for config.
- All three faces — CLI, UI, Terraform — call this API.
- Direct SQL writes to control-plane PG are detected and emit `ControlPlaneOutOfBandWrite` warnings.
- Every admin write is transactional, versioned (monotonic `version` + `etag`), and audited.

### The CLI — `eh ctl`

- Distributed as a single Rust binary.
- Subcommands match resource verbs: `source`, `entity`, `binding`, `rule`, `agent`, `intent`, `policy`.
- For ops, scripting, CI/CD, on-call surgery.

### The React UI

- Static SPA built with Vite + Tailwind + Tanstack Query + react-flow.
- Served by the binary on `/console/` or by a CDN.
- Auth via OIDC.
- Job: graphical editor and dashboards over the admin API.
  - Source management; physical schema tree explorer.
  - Drag-and-drop entity-binding designer with live type-check.
  - Routing-rule visualizer; "test an intent" lights up the path.
  - Live observability: top intents, cost per agent, slow queries, drift alerts.
  - **"Promote to git"** — exports current config as YAML, opens a PR. Closes the GitOps loop for teams not starting TF-first.

### The Terraform provider

- Go, `terraform-plugin-framework`, published to the Terraform Registry.
- Resources: `eh_source`, `eh_entity`, `eh_binding`, `eh_routing_rule`, `eh_agent`, `eh_capability`, `eh_policy`.
- Data sources: `eh_source`, `eh_entity` (read-only lookups, e.g., for probed physical schemas).
- Importable existing state (`terraform import`).
- Drift detection between TF state and EH config.

### The single contract

OpenAPI 3.1, generated from Rust types via `utoipa`, is the source of truth:

- `eh ctl` is generated/maintained against it.
- TF provider is generated against it.
- UI types via `openapi-typescript`.

No drift between faces.

---

## 14. Kubernetes Deployment

EventHorizon is built K8s-native. Stateless pods, durable state in managed Postgres.

### Pod shape

- Deployment, **not** StatefulSet, 3+ replicas.
- HPA on custom Prometheus metric `intents_per_second` with CPU fallback.
- Pod anti-affinity across nodes and zones.
- PDB `minAvailable: 50%`.
- Readiness probe: pings control DB and each enabled source connector. Out of LB until ready.
- Liveness probe: process-alive only. Backend failures must not kill pods.
- Startup probe with generous timeout (DataFusion can take seconds to initialize Iceberg catalogs).
- Graceful SIGTERM: stop accepting, drain in-flight, exit within `terminationGracePeriodSeconds`.

### Latency budget (V1 targets)

| Phase | p99 |
| --- | --- |
| Routing decision (in-mem) | < 100 µs |
| Authz (Cedar, cached) | < 1 ms |
| Intent → plan compile | < 2 ms |
| Postgres point-read e2e | < 20 ms |
| Iceberg analytical scan e2e | < 10 s (cost-bounded) |
| **EventHorizon overhead vs raw backend** | **< 1 ms** |

### Resilience

- Circuit breakers per source (closed / half-open / open). Open → fall back per declarative rules.
- Background health checker per source; never on the request path.
- Bulkheading: slow Iceberg cannot starve PG point-reads (separate semaphore pools).
- Tokio runtime tuned to pod CPU limit; `mimalloc` or `jemalloc` allocator.

### Helm chart (shipped in `examples/helm/`)

- ConfigMap for static config; Secret for credentials.
- ServiceMonitor for Prometheus.
- HorizontalPodAutoscaler.
- PodDisruptionBudget.
- NetworkPolicy templates.
- Sample Ingress.

---

## 15. Security Model

### Threat model (the honest version)

- A compromised agent may attempt to read or modify data outside its capability set.
- A compromised agent may attempt schema reconnaissance.
- A community connector may have bugs or be malicious.
- Backend credentials may be exfiltrated if the gateway is compromised.

### Mitigations

| Threat | Mitigation |
| --- | --- |
| Agent attempts unauthorized read | Cedar policy denies before plan compilation; emits `IntentAuthorized { decision: Deny }` |
| Agent attempts destructive op | `update`/`delete` capability is per-binding; absent by default |
| Schema reconnaissance | Agent receives only declared entities; physical schema never exposed |
| Prompt-injected SQL | Agents cannot author SQL — the protocol is structured JSON intents |
| Malicious connector | Conformance suite, capability declaration, audit log of connector versions, `trusted_connectors_only` strict mode |
| Backend credential theft | Secrets stored as references (k8s Secret, Vault, AWS SM); never in control PG values |
| Identity spoofing | Mutual TLS or signed agent tokens; agent identity propagated via `SET LOCAL` to backend |
| Tampered audit log | Append-only PG table + optional S3 immutable archive |

### What we **do not** claim

- "Mathematically impossible" guarantees.
- "Zero-trust" as a marketing term.
- Defense against a compromised gateway process.

The product is **a tightly scoped, structured, audited, cost-bounded API surface** — not a security panacea.

---

## 16. Cross-Cutting Principles

These are load-bearing. They constrain every future decision.

1. **One contract, three faces.** OpenAPI is the source of truth; CLI/UI/TF are generated against it.
2. **Config-as-truth.** All persistent state in control-plane PG; UI/CLI/TF write through the admin API.
3. **Hot path is sub-millisecond. Off-path is async.** LLMs and heavy analysis never sit in the request path.
4. **Telemetry is the spine.** Every component emits typed events; sinks attach as modules.
5. **Kernel does no I/O.** All I/O is in modules with explicit traits.
6. **Connectors return `TableProvider`s.** DataFusion does the heavy lifting; community connectors are small.
7. **Lifecycle stages are first-class.** Register → Probe → Bind → Shadow → Promote.
8. **Cost is a first-class concern.** Plan-cost estimation gates execution; quotas enforced at the gateway.
9. **The control plane is itself a logical data source.** Same `Connector` abstraction, separate capability namespace.
10. **Audit is durable; telemetry is best-effort.** Two contracts, two sinks.

---

## 17. V1 Scope & Non-Goals

### V1 — In scope

- Postgres + Iceberg + DuckDB connectors (first-party).
- MCP + REST edges.
- Postgres control plane.
- Cedar authorization with identity passthrough.
- DataFusion-based plan compilation with projection / predicate / aggregation pushdown.
- Telemetry event bus with OTel and PG-audit sinks.
- `eh ctl` CLI.
- Helm chart, basic Grafana dashboards.
- Five-stage connector onboarding lifecycle.

### V1 — Explicitly out

- gRPC edge (V1.1).
- React UI (V1.1; CLI is sufficient for early adopters).
- Terraform provider (V1.1).
- Kafka telemetry sink (V1.1).
- Snowflake / SQL Server first-party connectors (V1.1, after the API is proven).
- Async LLM copilot (V2).
- Artifact cache (V2).
- Write federation (V1 = read federation; writes pin to one source per entity).
- Cross-source transactional consistency (never — document the semantics, don't pretend).
- WASM connectors (V3+ if demand).
- Wire-protocol proxy mode (separate product, not a foundational decision).

---

## 18. Roadmap

| Milestone | Contents | Target |
| --- | --- | --- |
| **V0.1** | Workspace skeleton, `eh-core`, `eh-connector-api`, Postgres connector, MCP edge, in-mem control plane, `eh ctl` minimal | 4 weeks |
| **V0.2** | Postgres control plane, Cedar policy, DataFusion pipeline, Iceberg connector, OTel sink, cost gating | 8 weeks |
| **V0.3** | Lifecycle (register/probe/bind/shadow/promote), audit log, Helm chart, dashboards | 12 weeks |
| **V1.0** | Hardened V0.3 + REST edge, drift detector, conformance suite, public connector API | 16 weeks |
| **V1.1** | gRPC edge, React UI, Terraform provider, Kafka sink, Snowflake/SQL Server connectors | 24 weeks |
| **V2.0** | Async copilot, artifact cache, materialized-view recommendations, multi-tenant control plane | TBD |

---

## 19. Glossary

- **Intent** — a typed JSON request describing what the consumer wants, not how to get it. The smallest atomic unit of work.
- **Entity** — a logical, agent-facing concept (e.g., `Customer`). Maps to one or more physical bindings.
- **Binding** — the mapping of an entity (and its fields) to a specific table/path in a specific source.
- **Source** — a registered backend (a Postgres cluster, an Iceberg catalog, a Snowflake account).
- **Connector** — the Rust crate implementing the `Connector` trait for a specific source kind.
- **Routing rule** — a declarative `when → target` decision.
- **Ring** — a deployment tier (`staging` | `production`) controlling routing eligibility.
- **Profile** — a tag on a binding describing its workload character (`oltp`, `analytical`, `archival`).
- **Capability** — an `(agent, entity, action)` permission, evaluated via Cedar.
- **Artifact** — the token-dense JSON returned to the consumer.
- **Plan hash** — a stable hash of a compiled DataFusion plan, used for caching and audit.
- **Async copilot** — the optional LLM (e.g., Gemma) that reads the telemetry stream off the hot path and proposes optimizations.

---

## 20. Phased Implementation Plan

This section decomposes the §18 roadmap milestones into 13 sequenced, landable phases (0–12), plus a 14th deferred to V2.0. Each phase corresponds to a single GitHub epic issue (see [ROADMAP.md](./ROADMAP.md) for the live link table) and contains multiple short-lived sub-task PRs.

The plan follows a **walking-skeleton** strategy: **Phase 1 delivers the First Viable Product (FVP)** — a deployable, testable end-to-end thin slice through every layer (MySQL connector + REST + CLI + container). Subsequent phases thicken each layer without ever returning `main` to a state that lacks a runnable artifact.

### Operating principles (binding)

- **Trunk-based.** All work integrates into `main` via short-lived topic branches and pull requests. Branch protection enforces "no direct push to `main`" and Mandate-5 CI green.
- **Atomic phases.** Each phase either lands fully (Mandate-5 gates green, feature operational end-to-end) or does not land at all. Partial features live behind feature flags.
- **Sequential execution.** Exactly one phase in flight at any time. The next phase does not begin until the prior phase's acceptance criteria are met on `main`.
- **Always-testable.** Every phase produces a runnable artifact and a smoke test the operator can execute on their own machine. No accumulation of dead code.
- **Everything parameterized.** YAML config (`eventhorizon.yaml`) drives sources, entities, bindings, routing rules; env vars supply secrets; no hardcoded constants in business logic.
- **Schema-first.** Any phase that touches the data schema produces a schema-design PR first, awaiting operator approval, before the implementation PR.
- **Single contract.** The OpenAPI spec generated from `eh-protocol` is the source of truth for the admin surface; CLI, UI, and TF provider are generated from it.

### Gates at every milestone

| Gate | What must be true |
| --- | --- |
| **Mandate-5 (every PR)** | `cargo fmt --check` clean · `cargo clippy --all-targets --all-features -- -D warnings` clean · `cargo test --workspace` green |
| **🎯 FVP Gate (end of Phase 1)** | `docker compose up` brings gateway + MySQL online; `eh ctl intent send` and `curl POST /v1/intent` both return typed JSON artifacts from MySQL |
| **V0.1 Gate (end of Phase 3)** | FVP + MCP edge + MySQL connector passes Conformance §1–§3 |
| **V0.2 Gate (end of Phase 9)** | Federated `Customer` entity (MySQL + Postgres + Iceberg) with Cedar authz, telemetry, cost gating |
| **V1.0 Gate (end of Phase 11)** | Helm-installable, drift-detecting, REST+MCP-edged, semver-stable connector API published |

### Phase index

| # | Name | Milestone | Window | Depends on |
| --- | --- | --- | --- | --- |
| 0  | Bootstrap (workspace + CI + Docker)                            | pre-V0.1 | Week 1      | —  |
| 1  | **Walking Skeleton FVP** (MySQL + REST + CLI + compose)        | V0.1     | Weeks 2–3   | 0  |
| 2  | `eh-edge-mcp`                                                  | V0.1     | Week 4      | 1  |
| 3  | Conformance suite + MySQL §1–§3                                | V0.1     | Week 5      | 2  |
| 4  | `eh-connector-postgres`                                        | V0.2     | Weeks 5–6   | 3  |
| 5  | `eh-policy` (Cedar) + identity passthrough                     | V0.2     | Weeks 6–7   | 4  |
| 6  | `eh-control-pg` (replaces YAML for live config)                | V0.2     | Weeks 7–9   | 5  |
| 7  | `eh-connector-iceberg`                                         | V0.2     | Weeks 9–11  | 6  |
| 8  | `eh-telemetry` + OTel + audit sinks                            | V0.2     | Weeks 11–12 | 7  |
| 9  | `eh-cost`                                                      | V0.2     | Weeks 12–13 | 8  |
| 10 | Connector lifecycle + `eh ctl` expansion                       | V0.3     | Weeks 13–15 | 9  |
| 11 | Drift detector + Helm + dashboards + V1.0 release              | V1.0     | Weeks 15–16 | 10 |
| 12 | V1.1 expansion (gRPC, UI, TF, Kafka, Snowflake/MSSQL)          | V1.1     | Weeks 16–24 | 11 |
| 13 | V2.0 async copilot + artifact cache + recommendations          | V2.0     | TBD         | 12 |

---

### Phase 0 — Bootstrap *(pre-V0.1, Week 1)*

**Goal.** Workspace, CI gates, container build, and `docker compose` topology — all in place with a no-op binary.

**Deliverables**
- Cargo workspace with empty crate stubs per [Appendix A](#appendix-a--crate-layout).
- CI: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test --workspace`.
- Pre-commit hooks (same gates).
- **Multi-stage Dockerfile** (Rust build → minimal distroless or alpine runtime).
- **`docker-compose.yml`** with two services: `eventhorizon` (built from Dockerfile) + `mysql` (official MySQL 8.0 image) on a bridged network, volumes, env-injected credentials.
- `examples/config/eventhorizon.yaml` (empty schema, validated).
- `LICENSE` (Apache-2.0), `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`.
- `/docs/` directory; `README.md` links to this document.
- Branch protection on `main` (require PR, require CI green).

**Acceptance.** `docker compose build` succeeds. `docker compose up` brings both services up. The no-op binary serves `GET /healthz` returning 200. `cargo test --workspace` green on a fresh clone. Branch protection rejects direct pushes to `main`.

---

### Phase 1 — Walking Skeleton FVP 🎯 *(V0.1, Weeks 2–3)*

**Goal.** End-to-end thin slice. A deployable container that reads a parameterized YAML config, connects to MySQL via a real connector, accepts intents over REST and CLI, and returns JSON artifacts. **This is the First Viable Product.**

**Deliverables (intentionally minimal — each layer thin but real)**
- `eh-core`: `Intent`, `Entity`, `Binding`, `Artifact`, `CallerContext`, `Action`, `Mode` — just enough for read intents.
- `eh-protocol`: JSON Schema for `Intent` + REST contract.
- `eh-config`: YAML loader for sources, entities, bindings, and a minimal routing table. Validated against schema; hot-reloadable on SIGHUP. Secrets via env-var references only (`${ENV:MYSQL_PASSWORD}`).
- `eh-connector-api`: trait + DataFusion `TableProvider` re-export (minimal surface).
- `eh-connector-mysql`: **read-only**, parameterized statements via `sqlx::MySql`, exposes a DataFusion `TableProvider`. No writes yet.
- `eh-compiler`: DataFusion-backed compilation of Intent → `LogicalPlan` (projection + filter only).
- `eh-router`: trivial map lookup against the loaded YAML.
- `eh-edge-rest`: `POST /v1/intent`, `GET /v1/entities`, `GET /healthz`.
- `eh-ctl` CLI: `eh ctl start`, `eh ctl config validate <file>`, `eh ctl intent send <file-or-stdin>`, `eh ctl health`.
- `eh-bin`: wires it all together; reads `eventhorizon.yaml` from `--config` flag or `EH_CONFIG` env var.
- `examples/compose/`: docker-compose with MySQL 8.0 pre-seeded via init script (`customers` table + 10 rows).
- `examples/intents/`: sample intent JSON files for `Customer.read`.
- `make smoke` (or `just smoke`): brings stack up, sends a sample intent, asserts shape of response.

**Depends on.** Phase 0.
**Acceptance (the FVP gate)**
1. `docker compose up` succeeds.
2. `eh ctl intent send examples/intents/customer-point-read.json` returns a typed JSON artifact from MySQL.
3. `curl -X POST http://localhost:8080/v1/intent -d @examples/intents/customer-point-read.json` returns the same.
4. Edit `eventhorizon.yaml`, send `SIGHUP` to the gateway, new binding takes effect without restart.
5. `cargo test --workspace` green; `cargo clippy --all-targets --all-features -- -D warnings` clean.

> **🎯 FVP Gate — End of Phase 1 (≈ Week 3).** You can run it. You can test it. Every subsequent phase thickens this skeleton; nothing accumulates untested.

---

### Phase 2 — `eh-edge-mcp` *(V0.1, Week 4)*

**Goal.** Add the MCP edge alongside REST — the agent-facing protocol promised in §6.

**Deliverables**
- `eh-edge-mcp`: per-entity MCP tools auto-generated from the entity schema.
- Same pipeline as REST; just a different edge.
- Sample Claude Desktop config in `examples/`.

**Depends on.** Phase 1.
**Acceptance.** Claude Desktop (or any MCP client) connects, discovers `customer.read`, calls it, gets the artifact. Same intent works via REST and MCP indistinguishably.

---

### Phase 3 — Conformance suite + MySQL §1–§3 *(V0.1, Week 5)*

**Goal.** Codify the connector contract before adding a second connector. Lock the MySQL connector to it.

**Deliverables**
- `eh-conformance` suite: §1 (Capability honesty), §2 (Schema introspection), §3 (CRUD semantics for `read`).
- Property-based pushdown-correctness tests.
- MySQL connector passes §1–§3.
- Connector starter template (`cargo-generate`).

**Depends on.** Phase 2.
**Acceptance.** `cargo test -p eh-conformance -- --connector eh-connector-mysql` green against the dockerized MySQL.

> **🎯 V0.1 Gate — End of Phase 3.** FVP + MCP + Conformance. The shape of the product is proven.

---

### Phase 4 — `eh-connector-postgres` *(V0.2, Weeks 5–6)*

**Goal.** Second connector. Proves the Connector API is real and not MySQL-shaped by accident.

**Deliverables**
- `eh-connector-postgres` via `sqlx::Postgres`, identical capability surface to MySQL.
- Docker-compose gains a `postgres` service.
- Conformance §1–§3 green for Postgres.
- Example: same `Customer` entity bound to either MySQL or Postgres per YAML config.

**Depends on.** Phase 3.
**Acceptance.** Same intent against the same `Customer` entity routes to either MySQL or Postgres based on YAML config — the agent never sees the difference.

---

### Phase 5 — `eh-policy` (Cedar) + identity passthrough *(V0.2, Weeks 6–7)*

**Goal.** Authorization on the hot path; backend identity passthrough for RLS.

**Deliverables**
- `eh-policy`: Cedar wrapper, policy cache, decision API.
- Policy file loader (YAML for now; PG-backed in Phase 6).
- Identity passthrough hooks for MySQL (`SET @app_agent_id`) and Postgres (`SET LOCAL app.agent_id`).
- Sample RLS / row-permission fixtures in both connector tests.

**Depends on.** Phase 4.
**Acceptance.** Unauthorized agent denied before plan compilation. Authorized agent sees only rows it should (RLS / row-permission proof).

---

### Phase 6 — `eh-control-pg` (replaces YAML for live config) *(V0.2, Weeks 7–9)*

**Goal.** Promote from YAML-file control plane to durable Postgres-backed control plane. The YAML loader remains for tests + offline tooling.

**Deliverables**
- `eh-control-api` trait + `eh-control-pg` impl + schema (schema-first per §11).
- `ArcSwap<CompiledConfig>` + `LISTEN/NOTIFY` refresh; 60 s safety reload.
- Database accounts per §10/§13: app uses `eh_service` with `SELECT`+`INSERT` only; admin reserved for operator-applied migrations.
- Migration path: `eh ctl config import` ingests existing YAML config into PG.

**Depends on.** Phase 5.
**Acceptance.** Change a binding via admin SQL → all pods reflect within 1 s.

---

### Phase 7 — `eh-connector-iceberg` *(V0.2, Weeks 9–11)*

**Goal.** First analytical / lakehouse connector. Federation becomes real.

**Deliverables**
- `eh-connector-iceberg` via `iceberg-rust`.
- DataFusion `TableProvider` with predicate pushdown + partition pruning + projection pushdown.
- Conformance Suite §1–§4 green.
- E2E: `Customer` entity bound to both MySQL/Postgres (oltp) and Iceberg (analytical); router picks correctly.

**Depends on.** Phase 6.
**Acceptance.** Trend-intent over 90 d routes to Iceberg, returns aggregated artifact in <2 s; point-intent routes to OLTP in <20 ms.

---

### Phase 8 — `eh-telemetry` + OTel + audit sinks *(V0.2, Weeks 11–12)*

**Goal.** The spine. Every component emits typed events; OTel traces + durable audit.

**Deliverables**
- `eh-telemetry`: `TelemetryEvent` enum (per §10), event bus, `Sink` trait.
- `eh-sink-otlp`: OTel span emission.
- `eh-sink-pg-audit`: transactional audit-row write.
- Bounded buffers; drop metrics on sink failure.

**Depends on.** Phase 7.
**Acceptance.** Every intent produces a closed OTel span tree in Tempo/Jaeger; every intent produces an audit row before HTTP response returns.

---

### Phase 9 — `eh-cost` *(V0.2, Weeks 12–13)*

**Goal.** Cost-bounded execution. Don't hammer BigQuery / Iceberg / Snowflake.

**Deliverables**
- `eh-cost`: budgets, quotas (per agent / per source), plan-cost estimator using DataFusion plan stats.
- Pre-execution gating; structured `IntentRejected` errors with `retry_after_ms`.
- Circuit breakers per source.
- Adaptive throttling task (async).

**Depends on.** Phase 8.
**Acceptance.** Over-budget intent rejected before execution with a structured error. Circuit-breaker open redirects per declarative fallback.

> **🎯 V0.2 Gate — End of Phase 9.** Federated, policy-governed, cost-bounded, observable.

---

### Phase 10 — Connector lifecycle + `eh ctl` expansion *(V0.3, Weeks 13–15)*

**Goal.** The Register → Probe → Bind → Shadow → Promote workflow becomes a first-class CLI surface.

**Deliverables**
- `eh ctl` subcommands: `source add|probe|promote`, `entity bind`, `intent test --explain`, `rule add`.
- Per-source `status` state machine in control plane.
- Shadow-traffic execution (configurable percent).
- Rollback at every step.

**Depends on.** Phase 9.
**Acceptance.** Adding a new Iceberg source end-to-end through all 5 stages takes <15 minutes following CLI prompts; final routing rule applies in <1 s after `promote`.

---

### Phase 11 — Drift detector + Helm + dashboards + V1.0 release *(V1.0, Weeks 15–16)*

**Goal.** Production-grade V1.0 release. REST already exists since Phase 1; this phase hardens, adds drift + Helm + dashboards, and stabilizes the public connector API.

**Deliverables**
- Drift detector: per-source background task; `SourceDrift` events; warn / fail_closed / shadow policy.
- Helm chart (`examples/helm/`): Deployment, HPA, PDB, ServiceMonitor, NetworkPolicy.
- Grafana dashboards (`examples/dashboards/`): cost per agent / entity / source; top-N intents; drift; circuit-breaker state.
- Conformance Suite §1–§8 complete; "verified" badge automation.
- `eh-connector-api` semver-stabilized; published to crates.io.

**Depends on.** Phase 10.
**Acceptance.** Helm install → MCP probe → REST probe → both green. Drift detector flags an intentionally-mutated backend column within one detection interval.

> **🎯 V1.0 Gate — End of Phase 11.** Public release. Public connector API. First-party MySQL + Postgres + Iceberg. MCP + REST edges. Hardened control plane.

---

### Phase 12 — V1.1 expansion *(V1.1, Weeks 16–24)*

**Goal.** Operator-experience and ecosystem expansion. Decomposed into four sub-phases, sequenced.

- **12a — `eh-edge-grpc`** (1 week): gRPC parity with REST; same OpenAPI-derived contract.
- **12b — Terraform provider** (2 weeks): Go, `terraform-plugin-framework`, separate repo, resources `eh_source / eh_entity / eh_binding / eh_routing_rule / eh_agent / eh_capability / eh_policy`.
- **12c — React UI console** (3 weeks): Vite + Tailwind + Tanstack Query + react-flow; same admin API; "Promote to git" PR-emission flow.
- **12d — `eh-sink-kafka` + community connectors** (2 weeks): `rdkafka`, schema-registry-compatible Avro/Protobuf, topics `eh.intent.v1` / `eh.health.v1` / `eh.drift.v1`. First-class Snowflake (ADBC) + SQL Server (`tiberius`) connectors.

**Depends on.** Phase 11.
**Acceptance.** Each sub-phase Mandate-5 green at its own milestone.

---

### Phase 13 — V2.0 async copilot + artifact cache + recommendations *(V2.0, TBD)*

**Goal.** Off-hot-path intelligence. Operator copilot watches telemetry and proposes optimizations as PR-shaped recommendations.

**Deliverables**
- Telemetry-consumer service (separate deployment).
- Gemma integration via Candle (out-of-process).
- Materialized-view proposer; routing-rule recommender; cost-saving suggestions.
- Artifact cache (intent-hash + source-watermark keyed).
- Multi-tenant control plane.

**Depends on.** Phase 12.
**Acceptance.** A recurring expensive intent produces a materialized-view proposal in the UI within 24 h of pattern emergence.

---

### Cross-link map

| Artifact | Lives at | Cross-references |
| --- | --- | --- |
| This document (the spec) | repo `main` + local | Each phase block above links to its issue via [ROADMAP.md](./ROADMAP.md) |
| `ROADMAP.md` | repo root | Index of phases → issues → architecture sections |
| GitHub issue per phase | `k8nstantin/eventhorizon` Issues | Anchor link back to the phase block in this document |
| `README.md` | repo root | Elevator pitch + link to this doc + ROADMAP + rendered HTML |
| Rendered `index.html` | served via GitHub Pages | Same content as this `.md`, fetched and rendered client-side |

### Standard issue template (used for every phase)

```
**Phase N — <Name>**

Architecture reference: [§20 / Phase N](link)
Depends on: #issue-of-prior-phase
Target milestone: V0.X

## Goal
<one sentence>

## Deliverables
- [ ] component 1
- [ ] component 2
…

## Acceptance criteria (Mandate-5)
- [ ] cargo test --workspace green
- [ ] cargo clippy --all-targets --all-features -- -D warnings green
- [ ] <functional gate>

## Sub-task PRs
(added as work begins; each is a small, short-lived branch)
```

---

## Appendix A — Crate Layout

See [§3](#3-the-kernel--module-system). The full `Cargo.toml` for `eh-bin` will look like:

```toml
[package]
name = "eh-bin"
version = "0.1.0"
edition = "2021"

[dependencies]
eh-core             = { path = "../eh-core" }
eh-protocol         = { path = "../eh-protocol" }
eh-connector-api    = { path = "../eh-connector-api" }
eh-control-api      = { path = "../eh-control-api" }
eh-policy           = { path = "../eh-policy" }
eh-router           = { path = "../eh-router" }
eh-compiler         = { path = "../eh-compiler" }
eh-telemetry        = { path = "../eh-telemetry" }
eh-cost             = { path = "../eh-cost" }

eh-edge-mcp         = { path = "../eh-edge-mcp",  optional = true }
eh-edge-rest        = { path = "../eh-edge-rest", optional = true }
eh-edge-grpc        = { path = "../eh-edge-grpc", optional = true }

eh-control-pg       = { path = "../eh-control-pg", optional = true }
eh-sink-otlp        = { path = "../eh-sink-otlp",  optional = true }
eh-sink-kafka       = { path = "../eh-sink-kafka", optional = true }

eh-connector-postgres = { path = "../eh-connector-postgres", optional = true }
eh-connector-iceberg  = { path = "../eh-connector-iceberg",  optional = true }
eh-connector-duckdb   = { path = "../eh-connector-duckdb",   optional = true }
eh-connector-mysql    = { path = "../eh-connector-mysql",    optional = true }

tokio = { version = "1", features = ["full"] }
clap  = { version = "4", features = ["derive"] }

[features]
default = [
  "edge-mcp", "edge-rest",
  "control-pg",
  "sink-otlp",
  "connector-postgres", "connector-iceberg"
]
edge-mcp           = ["dep:eh-edge-mcp"]
edge-rest          = ["dep:eh-edge-rest"]
edge-grpc          = ["dep:eh-edge-grpc"]
control-pg         = ["dep:eh-control-pg"]
sink-otlp          = ["dep:eh-sink-otlp"]
sink-kafka         = ["dep:eh-sink-kafka"]
connector-postgres = ["dep:eh-connector-postgres"]
connector-iceberg  = ["dep:eh-connector-iceberg"]
connector-duckdb   = ["dep:eh-connector-duckdb"]
connector-mysql    = ["dep:eh-connector-mysql"]
all-connectors     = ["connector-postgres", "connector-iceberg", "connector-duckdb", "connector-mysql"]
slim               = ["edge-mcp", "control-pg", "connector-postgres"]
```

---

## Appendix B — Worked Example: Federated Customer Entity

A `Customer` entity served by Postgres (OLTP, latest state) and Iceberg (history, analytical).

### Entity schema (YAML, applied via `eh ctl apply`)

```yaml
entity: Customer
version: 3
fields:
  - { name: id,         type: string, required: true,  pii: false }
  - { name: name,       type: string, required: true,  pii: true  }
  - { name: email,      type: string, required: true,  pii: true  }
  - { name: signup_at,  type: timestamp,                pii: false }
  - { name: ltv,        type: decimal(12,2),            pii: false }

sources:
  pg_oltp:
    kind: postgres
    table: public.customers
    supports: [read, append, update]
    profile: oltp
    field_map:
      id: customer_id
      name: full_name
      email: email
      signup_at: created_at
      ltv: lifetime_value_usd
  ice_history:
    kind: iceberg
    table: warehouse.customers_history
    supports: [read]
    profile: analytical
    partition_hints: [{ field: signup_at, granularity: day }]
    field_map:
      id: customer_id
      name: full_name
      email: email
      signup_at: signup_ts
      ltv: ltv_usd

routing:
  - when: { action: append }                                  → pg_oltp
  - when: { action: update }                                  → pg_oltp
  - when: { action: read, mode: point }                       → pg_oltp
  - when: { action: read, mode: [trend, aggregate, window] }  → ice_history
  - when: { action: read, window: ">=30d" }                   → ice_history
  - default                                                    → pg_oltp
```

### A trend intent

```json
{
  "agent_token": "...",
  "action": "read",
  "entity": "Customer",
  "mode": "trend",
  "window": "90d",
  "group_by": ["signup_at:month"],
  "metrics": ["count(*) as new_customers", "sum(ltv) as total_ltv"],
  "fields": ["signup_at", "new_customers", "total_ltv"]
}
```

### The pipeline

1. **Validation** — fields exist, types compatible.
2. **Authz** — Cedar: `permit (analyst, read, Customer)` matches.
3. **Routing** — `mode: trend` → `ice_history`.
4. **Plan compilation** — DataFusion plan against `iceberg_table_provider(warehouse.customers_history)` with projection `[signup_ts, ltv_usd]`, filter `signup_ts >= now() - 90d`, aggregation by month.
5. **Cost gating** — est_bytes = 18 MB; agent budget remaining; OK.
6. **Execution** — Iceberg manifest pruning yields 3 partitions; scan + aggregate.
7. **Artifact** — 90 rows (one per day) → 3 rows (one per month) → compact JSON.
8. **Telemetry** — 8 events emitted; OTel span tree closes; Kafka batch flushes.

### A point intent

```json
{
  "agent_token": "...",
  "action": "read",
  "entity": "Customer",
  "mode": "point",
  "filter": { "id": "cust_42" },
  "fields": ["id", "name", "email", "ltv"]
}
```

Routes to `pg_oltp`. Plan = `SELECT customer_id, full_name, email, lifetime_value_usd FROM public.customers WHERE customer_id = $1` issued inside a transaction with `SET LOCAL app.agent_id = $2`. p99 < 5 ms.

---

## Appendix C — Connector Conformance Suite

Every connector must pass `eh-conformance` to earn the "verified" badge.

### Test categories

1. **Capability honesty** — declared `ConnectorCaps` matches behavior; pushed-down predicates produce identical results to non-pushed-down plans (property test).
2. **Schema introspection** — `describe()` returns stable, complete output across re-runs.
3. **CRUD semantics** — per declared support, basic append/read/update flows succeed.
4. **Identity passthrough** — when `identity_passthrough: true`, downstream session is correctly scoped.
5. **Cancellation** — async cancellation propagates within 100 ms.
6. **Resource discipline** — no FD or memory leak across 10k iterations.
7. **Error mapping** — backend errors map to a defined `ConnectError` taxonomy.
8. **Streaming** — when `streaming: true`, large result sets stream without OOM.

### Running

```bash
cargo test -p eh-conformance -- --connector eh-connector-postgres --backend-uri $DATABASE_URL
```

CI matrices run the suite across supported backend versions. A green run produces a signed conformance report committed to the connector repo and surfaced in the registry.

---

*Document version: v0.1 · maintained at [github.com/k8nstantin/eventhorizon](https://github.com/k8nstantin/eventhorizon)*
