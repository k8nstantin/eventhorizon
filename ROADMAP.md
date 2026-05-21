# EventHorizon — Roadmap

The architecture is captured in [eventhorizon_architecture.md](./eventhorizon_architecture.md). The roadmap below is the implementation plan; each phase tracks via a GitHub issue. Cross-link column is populated as issues are filed.

## Gates at every milestone

| Gate | What must be true |
| --- | --- |
| **Mandate-5 (every PR)** | `cargo fmt --check` clean · `cargo clippy --all-targets --all-features -- -D warnings` clean · `cargo test --workspace` green |
| **🎯 FVP Gate (end of Phase 1, ≈ Week 4)** | `docker compose up` brings gateway + MySQL online; `eh ctl intent send` and `curl POST /v1/intent` both return typed JSON artifacts from MySQL; Tier-1 telemetry visible in stdout JSON logs + Prometheus `/metrics` |
| **V0.1 Gate (end of Phase 3, ≈ Week 7)** | FVP + MCP edge + Tempo/Grafana overlay + MySQL connector passes Conformance §1–§3 |
| **V0.2 Gate (end of Phase 9, ≈ Week 15)** | Federated `Customer` entity (MySQL + Postgres + Iceberg) with Cedar authz, Tier-2 telemetry (durable audit + Kafka), cost gating |
| **V1.0 Gate (end of Phase 11, ≈ Week 18)** | Helm-installable, drift-detecting, REST+MCP-edged, semver-stable connector API published |

## Phase index

| # | Name | Milestone | Window | Issue | Architecture section |
| --- | --- | --- | --- | --- | --- |
| 0  | **Bootstrap + Schema Design** (workspace + CI + Docker + SCHEMA.md + schema.html + MySQL init) | pre-V0.1 | Weeks 1–2   | [#1](https://github.com/k8nstantin/eventhorizon/issues/1)  | [§20 / P0](./eventhorizon_architecture.md#phase-0--bootstrap--schema-design-pre-v01-weeks-12) |
| 1  | **Walking Skeleton FVP** (MySQL + REST + CLI + compose + Tier-1 telemetry — trimmed) 🎯 | V0.1     | Weeks 3–4   | [#2](https://github.com/k8nstantin/eventhorizon/issues/2)  | [§20 / P1](./eventhorizon_architecture.md#phase-1--walking-skeleton-fvp--tier-1-telemetry--v01-weeks-34) |
| 2  | `eh-edge-mcp` + FVP polish (OTLP + Tempo overlay + SIGHUP + discovery + ctl helpers) | V0.1     | Weeks 5–6   | [#3](https://github.com/k8nstantin/eventhorizon/issues/3)  | [§20 / P2](./eventhorizon_architecture.md#phase-2--eh-edge-mcp--fvp-polish-v01-weeks-56) |
| 3  | Conformance suite + MySQL §1–§3                              | V0.1     | Week 7      | [#4](https://github.com/k8nstantin/eventhorizon/issues/4)  | [§20 / P3](./eventhorizon_architecture.md#phase-3--conformance-suite--mysql-13-v01-week-5) |
| 4  | `eh-connector-postgres`                                      | V0.2     | Weeks 7–8   | [#5](https://github.com/k8nstantin/eventhorizon/issues/5)  | [§20 / P4](./eventhorizon_architecture.md#phase-4--eh-connector-postgres-v02-weeks-56) |
| 5  | `eh-policy` (Cedar) + identity passthrough                   | V0.2     | Weeks 8–9   | [#6](https://github.com/k8nstantin/eventhorizon/issues/6)  | [§20 / P5](./eventhorizon_architecture.md#phase-5--eh-policy-cedar--identity-passthrough-v02-weeks-67) |
| 6  | `eh-control-pg` (replaces YAML for live config)              | V0.2     | Weeks 9–11  | [#7](https://github.com/k8nstantin/eventhorizon/issues/7)  | [§20 / P6](./eventhorizon_architecture.md#phase-6--eh-control-pg-replaces-yaml-for-live-config-v02-weeks-79) |
| 7  | `eh-connector-iceberg`                                       | V0.2     | Weeks 11–13 | [#8](https://github.com/k8nstantin/eventhorizon/issues/8)  | [§20 / P7](./eventhorizon_architecture.md#phase-7--eh-connector-iceberg-v02-weeks-911) |
| 8  | **Tier-2 telemetry**: typed event bus + PG audit + Kafka     | V0.2     | Weeks 13–14 | [#9](https://github.com/k8nstantin/eventhorizon/issues/9)  | [§20 / P8](./eventhorizon_architecture.md#phase-8--eh-telemetry--otel--audit-sinks-v02-weeks-1112) |
| 9  | `eh-cost`                                                    | V0.2     | Weeks 14–15 | [#10](https://github.com/k8nstantin/eventhorizon/issues/10) | [§20 / P9](./eventhorizon_architecture.md#phase-9--eh-cost-v02-weeks-1213) |
| 10 | Connector lifecycle + `eh ctl` expansion                     | V0.3     | Weeks 15–17 | [#11](https://github.com/k8nstantin/eventhorizon/issues/11) | [§20 / P10](./eventhorizon_architecture.md#phase-10--connector-lifecycle--eh-ctl-expansion-v03-weeks-1315) |
| 11 | Drift detector + Helm + dashboards + V1.0 release            | V1.0     | Weeks 17–18 | [#12](https://github.com/k8nstantin/eventhorizon/issues/12) | [§20 / P11](./eventhorizon_architecture.md#phase-11--drift-detector--helm--dashboards--v10-release-v10-weeks-1516) |
| 12 | V1.1 expansion (gRPC, UI, TF, Snowflake/MSSQL)               | V1.1     | Weeks 18–26 | [#13](https://github.com/k8nstantin/eventhorizon/issues/13) | [§20 / P12](./eventhorizon_architecture.md#phase-12--v11-expansion-v11-weeks-1624) |
| 13 | V2.0 async copilot + artifact cache + recommendations        | V2.0     | TBD         | [#14](https://github.com/k8nstantin/eventhorizon/issues/14) _(deferred)_ | [§20 / P13](./eventhorizon_architecture.md#phase-13--v20-async-copilot--artifact-cache--recommendations-v20-tbd) |

## Documents

- **[Architecture Specification](./eventhorizon_architecture.md)** — the load-bearing reference.
- **[Schema Reference](./SCHEMA.md)** — the design contract for every table (DRAFT; per-table operator approval required).
- **[Rendered Architecture HTML](https://k8nstantin.github.io/eventhorizon/)** | **[Rendered Schema HTML](https://k8nstantin.github.io/eventhorizon/schema.html)**

## Operating policy

Every phase obeys the [zero-trust execution mandates](./.claude/skills/zero-trust-execution/SKILL.md): trunk-based, single open PR at a time, Mandate-5 gates green, schema-first / code-after, least-privilege service account, types end-to-end. Architecture §3 + §16 are binding.
