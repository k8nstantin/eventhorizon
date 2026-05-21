# db/postgres/seeds/

Optional, **operator-opt-in** seed data for the control plane. Applied **after** `schema.sql` and after `zz_set_role_passwords.sh`.

## Contract

- Seeds are pure **INSERT** statements into the locked schema. They do not alter structure.
- Seeds are written so they can be run multiple times safely (use `ON CONFLICT DO NOTHING` or guarded `WHERE NOT EXISTS`).
- Seeds belong in this directory **only** when the operator wants them. They are not mounted into the runtime container by default.
- Seeds may insert into the control plane **as the `eh_admin` role** because they include rows in `eh_control` (which `eh_service` cannot write to). The seed runner script handles role switching.

## Use cases

- **Sample MySQL connector registration** — pre-register `fvp_mysql` as a source so the operator gets a working demo immediately after `docker compose up`.
- **Default tenant** — insert a `tenants` row so single-tenant deployments aren't empty.
- **Example entities** — `Customer`, `Order` etc. wired to the sample MySQL connector for the FVP smoke test.

## File layout (planned)

```
seeds/
├── 01_default_tenant.sql           # tenants(name='default')
├── 02_sample_mysql_source.sql      # sources + source_mysql for the FVP MySQL container
├── 03_customer_entity.sql          # Customer entity + entity_fields
├── 04_customer_binding.sql         # entity_binding to the sample MySQL source
└── 05_demo_agent.sql               # demo agent with eh-service-level capability
```

These files do not yet exist. They land per-need as the FVP demo crystallises.

## How to enable

Mount this directory into the postgres container's init dir alongside `schema.sql`:

```yaml
postgres:
  volumes:
    - ./db/postgres/schema.sql:/docker-entrypoint-initdb.d/01_schema.sql:ro
    - ./db/postgres/init/zz_set_role_passwords.sh:/docker-entrypoint-initdb.d/zz_set_role_passwords.sh:ro
    # Optional — enable for demo mode:
    - ./db/postgres/seeds:/docker-entrypoint-initdb.d/seeds:ro
    - ./db/postgres/init/yy_apply_seeds.sh:/docker-entrypoint-initdb.d/yy_apply_seeds.sh:ro
```

The `yy_apply_seeds.sh` wrapper applies seed files using the `eh_admin` role.
