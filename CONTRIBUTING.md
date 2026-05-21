# Contributing to EventHorizon

Thanks for your interest. EventHorizon is built under a strict trunk-based-development model with mandatory verification gates. Please read this in full before opening a PR.

## Operating model

All implementation work follows the [zero-trust execution mandates](./.claude/skills/zero-trust-execution/SKILL.md). The load-bearing rules:

- **Trunk-based.** One trunk: `main`. No `develop` branch, no long-running release branches. All work integrates to `main` via short-lived topic branches and pull requests.
- **Always-releasable.** `main` is always green. Every commit on `main` passes the full gate suite.
- **One open branch + one open PR at a time.** Sequential, not parallel.
- **Small diffs.** One logical step per PR. If two changes can be reasoned about independently, ship two PRs.

## Mandate-5 gates (every PR must pass these)

```
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
bash scripts/check-schema-sync.sh
```

CI enforces all four. PRs that do not pass them are not eligible for merge.

## Schema-first / code-after (zero-trust §11)

The data schema is **load-bearing architecture**, not implementation detail.

1. **Design first.** Any schema change (new table, new column, new constraint, new grant, new RLS policy, new index — *anything*) starts as a PR that updates [`SCHEMA.md`](./SCHEMA.md) and the matching DDL in [`db/`](./db/) together. Code does not change in this PR.
2. **Operator approves per-change.** Approval for one change does not extend to adjacent changes. Each new schema mutation is its own conversation.
3. **Operator applies the migration.** The application never authenticates as `eh_admin`. Migrations run under operator credentials.
4. **Code follows.** Application code is updated in a subsequent PR against the now-locked schema, authenticating as `eh_service` (`SELECT` on `eh_control`, `SELECT + INSERT` on `eh_operational`; no `UPDATE` / `DELETE` / DDL).

The CI guardrail (`scripts/check-schema-sync.sh`) enforces that `SCHEMA.md` and the DDL agree.

## Branch naming

- `feat/<thing>` — new functionality
- `fix/<thing>` — bug fix
- `docs/<thing>` — documentation-only changes
- `chore/<thing>` — tooling, CI, dependency bumps

## Commit messages

Imperative present tense, body explaining the *why*. Conventional-Commits prefix optional but appreciated:
```
feat(eh-connector-mysql): bind UUIDv7 to BINARY(16) without string coercion

The previous code went through UUID_TO_BIN() on every insert. sqlx already
supports direct binding when the column is BINARY(16); the shim was
unnecessary and broke §14 (no type conversions).
```

## What requires explicit operator approval (not auto-approval)

Per zero-trust §3 / §7 / §10–§13 / §16, the following changes require explicit, per-change operator sign-off:

- Any schema mutation (covered above).
- Any change to the architecture document (`eventhorizon_architecture.md`).
- Any change to grants (the `eh_service` privilege set).
- Any change to RLS policies.
- Any introduction of `UPDATE`, `DELETE`, `UPSERT`, or DDL on the application code path.
- Any new dependency in the workspace.
- Any use of `unsafe { }`.
- Any change that introduces a JSONB column outside the existing opaque-payload set.

## Pull-request checklist

Before requesting review:

- [ ] Branch is short-lived (under one day old).
- [ ] One logical step. Unrelated changes go in separate PRs.
- [ ] Mandate-5 gates green locally.
- [ ] Schema-sync check green (if you touched `db/` or `SCHEMA.md`).
- [ ] PR description states the *why* and how it was verified.

## Code of conduct

Participation is governed by the [Contributor Covenant](./CODE_OF_CONDUCT.md).
