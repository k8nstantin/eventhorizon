# Service-Account Binding

> EventHorizon application code authenticates to **our own resources** (control plane + FVP demo data source) only as `eh_service`. **Never** as `eh_admin`, `postgres` (PG superuser), `root` (FVP MySQL superuser), or any other admin role. SELECT and INSERT are the entire SQL vocabulary the application is allowed.
>
> This is a binding contract enforced by both the database engine (grants + RLS) and CI (`scripts/check-no-admin-creds.sh`).

## Scope clarification — operator-configured connector backends

This contract is about **EventHorizon's own identity** to:

1. Its **control plane** Postgres (`eh_service` role with the locked grants).
2. The **FVP-shipped MySQL demo source** (`eh_service` MySQL user with SELECT only on `eh_demo.customers`).

It is **NOT** about restricting what credentials an operator chooses to configure their own connector backends with. An operator who deploys EventHorizon against their tenant Postgres and writes:

```yaml
sources:
  my_tenant_pg:
    kind: postgres
    username: root
    password: ${ENV:MY_TENANT_PG_PASSWORD}
```

…is exercising legitimate choice. The connector code reads `username` from the YAML config and connects with whatever the operator supplied. **Connector credentials are operator-controlled at runtime, not Rust-hardcoded.**

The CI guardrail catches *hardcoded* references to our specific admin tokens in Rust source — it does not interfere with operator-supplied connector config.

Reference: [zero-trust §10 / §13](./.claude/skills/zero-trust-execution/SKILL.md), [SCHEMA.md §5](./SCHEMA.md#5-accounts--grants), [architecture §5.10](./eventhorizon_architecture.md#510-account--grant-model), [CONNECTORS.md](./CONNECTORS.md#security--governance-for-connector-authors).

---

## The grant model

| Role | `eh_control` | `eh_operational` | Other schemas |
|---|---|---|---|
| `eh_admin` (operator-only) | full | full | (operator escape hatch) |
| **`eh_service`** (the app) | **`SELECT` only** | **`SELECT` + `INSERT` only** | no `USAGE` |
| `postgres` (PG superuser) | (init only) | (init only) | (operator emergency) |

Anything the application attempts outside `eh_service`'s grant set — `UPDATE`, `DELETE`, `UPSERT`, `TRUNCATE`, DDL, cross-schema access — is **engine-refused** by Postgres itself. The refusal surfaces as a typed error; that error is the [§12 debugging surface](./.claude/skills/zero-trust-execution/SKILL.md) working as designed.

## FVP-shipped MySQL data source

For the demo MySQL the FVP ships in `docker-compose.yml`, the same lockdown applies:

| Role | `eh_demo.customers` | Other schemas |
|---|---|---|
| `eh_admin` (operator-only on FVP MySQL too) | full | full |
| **`eh_service`** (the app on FVP MySQL) | **`SELECT` only** (Phase 1; widens when `APPEND` lands per §11) | none |

This is **our** demo connector backend. Operator-deployed tenant connector backends are governed by the operator's own credential choices (see "Scope clarification" above).

Add-a-new-connector contributors: see [CONNECTORS.md](./CONNECTORS.md#security--governance-for-connector-authors).

## What this means for application code

Application code MUST:

- Read its DB credentials from env-injected secret refs supplied by the operator's secrets manager (Vault, AWS SM, GCP SM, k8s Secret, …).
- Issue **only** `SELECT` against `eh_control` and **only** `SELECT` + `INSERT` against `eh_operational`.
- Treat engine refusals as **bug signals** — log them, surface them, fix the application code that issued the refused operation.

Application code MUST NOT:

- Hold or reference admin credentials.
- Open a connection under any admin role.
- Switch roles mid-session to bypass a refusal.
- Issue `UPDATE`, `DELETE`, `UPSERT`, `TRUNCATE`, or any DDL against `eh_control` / `eh_operational`.
- Embed credentials as string literals in source.

## How this is enforced

1. **Engine grants.** `eh_service`'s privileges in Postgres / MySQL are set by the locked schema; the engine refuses anything outside them. See `db/postgres/schema.sql` + `db/postgres/init/zz_set_role_passwords.sh`, and `db/mysql/init/01_fvp_schema.sql` + `02_users.sh`.
2. **Lockdown verification.** `scripts/verify-control-plane-lockdown.sh` spins fresh Postgres, applies `schema.sql`, attaches passwords, and asserts engine refusal for every forbidden operation. Runs in CI on every PR.
3. **No-admin-creds CI guardrail.** `scripts/check-no-admin-creds.sh` greps `crates/**/*.rs` for any reference to admin / root / superuser identifiers or env-var names. PR fails if it finds any. Runs in CI on every PR.
4. **Code review.** Every PR explicitly checks the grant boundary as part of approval (CONTRIBUTING.md).

## Forbidden identifiers in `crates/**/*.rs`

The CI grep guardrail looks for these fixed patterns:

| Pattern | Why forbidden |
|---|---|
| `eh_admin` | The operator's admin role on Postgres / MySQL — never used by app code |
| `EH_ADMIN_PASSWORD` | Admin password env-var name — only the operator's role-setup scripts read this |
| `POSTGRES_ROOT_PASSWORD` | PG superuser password env-var name — entrypoint init only |
| `MYSQL_ROOT_PASSWORD` | MySQL superuser password env-var name — entrypoint init only |
| `MYSQL_EH_ADMIN_PASSWORD` | MySQL admin role password env-var name — operator scripts only |
| `postgres://postgres@` | PG superuser connection string |
| `mysql://root@` | MySQL superuser connection string |

If application code legitimately needs to reference one of these (it doesn't — that's the point), the answer is to file an issue for design review per [zero-trust §11](./.claude/skills/zero-trust-execution/SKILL.md), not to add an exception to the guardrail.

## The escalation path when the app needs something `eh_service` can't do

This is the **only** correct flow:

1. **Stop.** The refusal is correct; the code's intent isn't.
2. **Surface it.** Open an issue describing the operation, the entity / table involved, and why the application thought it needed admin privilege.
3. **Discuss the right fix.**
   - If the operation is genuinely application-scope, refactor the code to use an `INSERT` (state changes are new rows per SCD2, never UPDATE/DELETE).
   - If the grant set needs to change, that's a per-change operator-approved schema PR (§11). The change must be principled — e.g., "Phase X adds `APPEND` capability for entity Y, so the binding's allowed actions extend by one row in `entity_binding_actions`."
   - If the schema needs a new table or column, that's a schema PR with operator approval.
4. **Land the schema change.** Operator merges and applies it under their admin role.
5. **Application code follows.** Now the operation `eh_service` can do legitimately works.

At no point does the application authenticate as anything but `eh_service`.

## In one sentence

**`eh_service` only. Engine refusals are signals, not obstacles. The operator owns admin; the application owns its grants.**
