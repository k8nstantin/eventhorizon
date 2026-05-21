# EventHorizon

> A modular, Rust-native semantic gateway between agentic (and human) consumers and a heterogeneous fleet of data sources.

**Status:** pre-V0.1 · architecture stabilized · implementation about to begin from Phase 0.

EventHorizon exposes a single typed surface (MCP / REST / gRPC), routes intents declaratively to the best-fit backend, compiles them through Apache DataFusion with cost-bounded planning, returns token-dense artifacts, emits fine-grained telemetry to OTel / Kafka, and is fully manageable via CLI, Terraform, and a React console — all backed by the same admin API.

## Documents

- **[Architecture Specification](./eventhorizon_architecture.md)** — the load-bearing reference (§1–§19 + §20 phased plan + appendices).
- **[Schema Reference](./SCHEMA.md)** — the design contract for every table (DRAFT; per-table operator approval required per zero-trust §11).
- **[Roadmap](./ROADMAP.md)** — phase index with GitHub issue links.
- **Rendered HTML**: [architecture](https://k8nstantin.github.io/eventhorizon/) · [schema](https://k8nstantin.github.io/eventhorizon/schema.html) (served via GitHub Pages).

## First Viable Product (FVP)

The FVP arrives at the **end of Phase 1, ≈ Week 3**: a deployable container that you can `docker compose up` on Mac / Linux / VM, hitting a real MySQL backend via a real connector, with both a REST endpoint and a CLI to send intents.

```bash
# (forthcoming after Phase 1 lands)
docker compose up -d
eh ctl intent send examples/intents/customer-point-read.json
# → typed JSON artifact from MySQL
```

## Operating policy

Development follows the [zero-trust execution mandates](./.claude/skills/zero-trust-execution/SKILL.md):

- Trunk-based, single open PR at a time, Mandate-5 gates green (`cargo fmt --check` + `cargo clippy -- -D warnings` + `cargo test --workspace`).
- Schema-first / code-after; schema changes require explicit operator approval.
- App connects as a least-privilege service account (`SELECT` + `INSERT` only); admin reserved for operator.
- Types flow end-to-end unchanged; no coercion away from the schema's contract.
- Every intent emits typed telemetry; durable audit is non-negotiable.

## License

Apache-2.0.
