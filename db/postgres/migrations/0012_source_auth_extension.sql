-- ============================================================================
-- 0012 — Source auth-kind extension (mysql, postgres)
-- ============================================================================
-- Bring source_mysql and source_postgres to the same auth_kind pattern that
-- source_snowflake / source_mssql / source_iceberg already use. Password is
-- no longer the only path — mTLS and IAM auth are first-class.
--
-- Per-kind required-field invariants are enforced by a shape CHECK so the
-- engine refuses invalid combinations at insert time (zero-trust §12).
--
-- Backward compatibility: auth_kind defaults to 'password', so existing
-- rows (none yet — schema not applied to a live store) and any future
-- password-style inserts continue to work without code changes.
--
-- Reference: SCHEMA.md §3.7.1, §3.7.2. Architecture §5.6.
-- ============================================================================

-- ============================================================================
-- 1. source_mysql
-- ============================================================================

ALTER TABLE eh_control.source_mysql
  ALTER COLUMN password_secret_ref DROP NOT NULL,
  ADD COLUMN auth_kind            TEXT NOT NULL DEFAULT 'password',
  ADD COLUMN tls_cert_secret_ref  TEXT,
  ADD COLUMN tls_key_secret_ref   TEXT,
  ADD COLUMN tls_ca_secret_ref    TEXT,
  ADD COLUMN iam_role_arn         TEXT,
  ADD COLUMN iam_service_account  TEXT;

ALTER TABLE eh_control.source_mysql
  ADD CONSTRAINT source_mysql_auth_kind_chk
    CHECK (auth_kind IN ('password','mtls','iam_aws','iam_gcp'));

ALTER TABLE eh_control.source_mysql
  ADD CONSTRAINT source_mysql_tls_secret_ref_scheme
    CHECK (
      (tls_cert_secret_ref IS NULL OR tls_cert_secret_ref ~ '^(vault|k8s|aws-sm|gcp-sm|file|env)://')
      AND
      (tls_key_secret_ref  IS NULL OR tls_key_secret_ref  ~ '^(vault|k8s|aws-sm|gcp-sm|file|env)://')
      AND
      (tls_ca_secret_ref   IS NULL OR tls_ca_secret_ref   ~ '^(vault|k8s|aws-sm|gcp-sm|file|env)://')
    );

ALTER TABLE eh_control.source_mysql
  ADD CONSTRAINT source_mysql_auth_shape_chk CHECK (
    (
      auth_kind = 'password'
      AND password_secret_ref IS NOT NULL
      AND tls_cert_secret_ref IS NULL
      AND tls_key_secret_ref  IS NULL
      AND iam_role_arn        IS NULL
      AND iam_service_account IS NULL
    )
    OR
    (
      auth_kind = 'mtls'
      AND password_secret_ref IS NULL
      AND tls_cert_secret_ref IS NOT NULL
      AND tls_key_secret_ref  IS NOT NULL
      AND iam_role_arn        IS NULL
      AND iam_service_account IS NULL
    )
    OR
    (
      auth_kind = 'iam_aws'
      AND password_secret_ref IS NULL
      AND tls_cert_secret_ref IS NULL
      AND tls_key_secret_ref  IS NULL
      AND iam_role_arn        IS NOT NULL
      AND iam_service_account IS NULL
    )
    OR
    (
      auth_kind = 'iam_gcp'
      AND password_secret_ref IS NULL
      AND tls_cert_secret_ref IS NULL
      AND tls_key_secret_ref  IS NULL
      AND iam_role_arn        IS NULL
      AND iam_service_account IS NOT NULL
    )
  );

COMMENT ON COLUMN eh_control.source_mysql.auth_kind
  IS 'Authentication mode: password (default) | mtls | iam_aws | iam_gcp. Extending this enum is an operator-approved migration.';
COMMENT ON COLUMN eh_control.source_mysql.tls_cert_secret_ref
  IS 'Secrets-manager reference to client TLS certificate (PEM) for auth_kind=mtls.';
COMMENT ON COLUMN eh_control.source_mysql.tls_key_secret_ref
  IS 'Secrets-manager reference to client TLS private key (PEM) for auth_kind=mtls.';
COMMENT ON COLUMN eh_control.source_mysql.tls_ca_secret_ref
  IS 'Secrets-manager reference to CA certificate for ssl_mode=verify_ca / verify_identity. Optional; orthogonal to auth_kind.';
COMMENT ON COLUMN eh_control.source_mysql.iam_role_arn
  IS 'AWS IAM role ARN for RDS IAM authentication when auth_kind=iam_aws.';
COMMENT ON COLUMN eh_control.source_mysql.iam_service_account
  IS 'GCP service account email for Cloud SQL IAM authentication when auth_kind=iam_gcp.';

-- ============================================================================
-- 2. source_postgres
-- ============================================================================

ALTER TABLE eh_control.source_postgres
  ALTER COLUMN password_secret_ref DROP NOT NULL,
  ADD COLUMN auth_kind            TEXT NOT NULL DEFAULT 'password',
  ADD COLUMN tls_cert_secret_ref  TEXT,
  ADD COLUMN tls_key_secret_ref   TEXT,
  ADD COLUMN tls_ca_secret_ref    TEXT,
  ADD COLUMN iam_role_arn         TEXT,
  ADD COLUMN iam_service_account  TEXT;

ALTER TABLE eh_control.source_postgres
  ADD CONSTRAINT source_postgres_auth_kind_chk
    CHECK (auth_kind IN ('password','mtls','iam_aws','iam_gcp'));

ALTER TABLE eh_control.source_postgres
  ADD CONSTRAINT source_postgres_tls_secret_ref_scheme
    CHECK (
      (tls_cert_secret_ref IS NULL OR tls_cert_secret_ref ~ '^(vault|k8s|aws-sm|gcp-sm|file|env)://')
      AND
      (tls_key_secret_ref  IS NULL OR tls_key_secret_ref  ~ '^(vault|k8s|aws-sm|gcp-sm|file|env)://')
      AND
      (tls_ca_secret_ref   IS NULL OR tls_ca_secret_ref   ~ '^(vault|k8s|aws-sm|gcp-sm|file|env)://')
    );

ALTER TABLE eh_control.source_postgres
  ADD CONSTRAINT source_postgres_auth_shape_chk CHECK (
    (
      auth_kind = 'password'
      AND password_secret_ref IS NOT NULL
      AND tls_cert_secret_ref IS NULL
      AND tls_key_secret_ref  IS NULL
      AND iam_role_arn        IS NULL
      AND iam_service_account IS NULL
    )
    OR
    (
      auth_kind = 'mtls'
      AND password_secret_ref IS NULL
      AND tls_cert_secret_ref IS NOT NULL
      AND tls_key_secret_ref  IS NOT NULL
      AND iam_role_arn        IS NULL
      AND iam_service_account IS NULL
    )
    OR
    (
      auth_kind = 'iam_aws'
      AND password_secret_ref IS NULL
      AND tls_cert_secret_ref IS NULL
      AND tls_key_secret_ref  IS NULL
      AND iam_role_arn        IS NOT NULL
      AND iam_service_account IS NULL
    )
    OR
    (
      auth_kind = 'iam_gcp'
      AND password_secret_ref IS NULL
      AND tls_cert_secret_ref IS NULL
      AND tls_key_secret_ref  IS NULL
      AND iam_role_arn        IS NULL
      AND iam_service_account IS NOT NULL
    )
  );

COMMENT ON COLUMN eh_control.source_postgres.auth_kind
  IS 'Authentication mode: password (default) | mtls | iam_aws | iam_gcp. Extending this enum is an operator-approved migration.';
COMMENT ON COLUMN eh_control.source_postgres.tls_cert_secret_ref
  IS 'Secrets-manager reference to client TLS certificate (PEM) for auth_kind=mtls.';
COMMENT ON COLUMN eh_control.source_postgres.tls_key_secret_ref
  IS 'Secrets-manager reference to client TLS private key (PEM) for auth_kind=mtls.';
COMMENT ON COLUMN eh_control.source_postgres.tls_ca_secret_ref
  IS 'Secrets-manager reference to CA certificate for ssl_mode=verify-ca / verify-full. Optional; orthogonal to auth_kind.';
COMMENT ON COLUMN eh_control.source_postgres.iam_role_arn
  IS 'AWS IAM role ARN for RDS IAM authentication when auth_kind=iam_aws.';
COMMENT ON COLUMN eh_control.source_postgres.iam_service_account
  IS 'GCP service account email for Cloud SQL IAM authentication when auth_kind=iam_gcp.';
