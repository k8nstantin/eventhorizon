-- ============================================================================
-- 0005 — Per-connector typed config tables (the ProxySQL pattern)
-- ============================================================================
-- One typed config table per connector kind. FK 1:1 to sources(id).
-- Adding a new connector kind = new migration adding source_<kind> +
-- extending sources.kind CHECK enum.
--
-- Reference: SCHEMA.md §3.7.
-- ============================================================================

-- Helper macro (textual): every source_<kind> table has the same SCD2 triad
-- and the same source_id PK+FK structure. Tables are written out explicitly
-- because Postgres doesn't have a clean template mechanism for table DDL.

-- ============================================================================
-- 1. source_mysql (Phase 1 FVP target)
-- ============================================================================

CREATE TABLE eh_control.source_mysql (
  source_id            UUID         NOT NULL,
  host                 TEXT         NOT NULL,
  port                 INT          NOT NULL DEFAULT 3306,
  database_name        TEXT         NOT NULL,
  username_secret_ref  TEXT         NOT NULL,
  password_secret_ref  TEXT         NOT NULL,
  ssl_mode             TEXT         NOT NULL DEFAULT 'preferred',
  max_pool_size        INT          NOT NULL DEFAULT 8,

  valid_from           TIMESTAMPTZ  NOT NULL DEFAULT now(),
  valid_to             TIMESTAMPTZ  NOT NULL DEFAULT 'infinity',
  is_current           BOOLEAN      NOT NULL DEFAULT true,

  PRIMARY KEY (source_id),
  CONSTRAINT source_mysql_source_fk
    FOREIGN KEY (source_id) REFERENCES eh_control.sources(id),
  CONSTRAINT source_mysql_port_range
    CHECK (port BETWEEN 1 AND 65535),
  CONSTRAINT source_mysql_pool_pos
    CHECK (max_pool_size > 0),
  CONSTRAINT source_mysql_ssl_chk
    CHECK (ssl_mode IN ('disabled','preferred','required','verify_ca','verify_identity')),
  CONSTRAINT source_mysql_secret_ref_username
    CHECK (username_secret_ref ~ '^(vault|k8s|aws-sm|gcp-sm|file|env)://'),
  CONSTRAINT source_mysql_secret_ref_password
    CHECK (password_secret_ref ~ '^(vault|k8s|aws-sm|gcp-sm|file|env)://'),
  CONSTRAINT source_mysql_valid_order
    CHECK (valid_to >= valid_from)
);

COMMENT ON TABLE eh_control.source_mysql
  IS 'MySQL connector config. 1:1 with sources where kind=''mysql''.';

-- ============================================================================
-- 2. source_postgres
-- ============================================================================

CREATE TABLE eh_control.source_postgres (
  source_id            UUID         NOT NULL,
  host                 TEXT         NOT NULL,
  port                 INT          NOT NULL DEFAULT 5432,
  database_name        TEXT         NOT NULL,
  username_secret_ref  TEXT         NOT NULL,
  password_secret_ref  TEXT         NOT NULL,
  application_name     TEXT         NOT NULL DEFAULT 'eventhorizon',
  ssl_mode             TEXT         NOT NULL DEFAULT 'require',
  max_pool_size        INT          NOT NULL DEFAULT 8,

  valid_from           TIMESTAMPTZ  NOT NULL DEFAULT now(),
  valid_to             TIMESTAMPTZ  NOT NULL DEFAULT 'infinity',
  is_current           BOOLEAN      NOT NULL DEFAULT true,

  PRIMARY KEY (source_id),
  CONSTRAINT source_postgres_source_fk
    FOREIGN KEY (source_id) REFERENCES eh_control.sources(id),
  CONSTRAINT source_postgres_port_range
    CHECK (port BETWEEN 1 AND 65535),
  CONSTRAINT source_postgres_pool_pos
    CHECK (max_pool_size > 0),
  CONSTRAINT source_postgres_ssl_chk
    CHECK (ssl_mode IN ('disable','allow','prefer','require','verify-ca','verify-full')),
  CONSTRAINT source_postgres_secret_ref_username
    CHECK (username_secret_ref ~ '^(vault|k8s|aws-sm|gcp-sm|file|env)://'),
  CONSTRAINT source_postgres_secret_ref_password
    CHECK (password_secret_ref ~ '^(vault|k8s|aws-sm|gcp-sm|file|env)://'),
  CONSTRAINT source_postgres_valid_order
    CHECK (valid_to >= valid_from)
);

COMMENT ON TABLE eh_control.source_postgres
  IS 'Postgres connector config. 1:1 with sources where kind=''postgres''.';

-- ============================================================================
-- 3. source_iceberg
-- ============================================================================

CREATE TABLE eh_control.source_iceberg (
  source_id          UUID         NOT NULL,
  catalog_uri        TEXT         NOT NULL,
  namespace          TEXT         NOT NULL,
  warehouse          TEXT         NOT NULL,
  auth_kind          TEXT         NOT NULL DEFAULT 'none',
  auth_secret_ref    TEXT,

  valid_from         TIMESTAMPTZ  NOT NULL DEFAULT now(),
  valid_to           TIMESTAMPTZ  NOT NULL DEFAULT 'infinity',
  is_current         BOOLEAN      NOT NULL DEFAULT true,

  PRIMARY KEY (source_id),
  CONSTRAINT source_iceberg_source_fk
    FOREIGN KEY (source_id) REFERENCES eh_control.sources(id),
  CONSTRAINT source_iceberg_auth_chk
    CHECK (auth_kind IN ('none','oauth','sigv4','token')),
  CONSTRAINT source_iceberg_secret_ref_match
    CHECK (
      (auth_kind = 'none' AND auth_secret_ref IS NULL)
      OR
      (auth_kind <> 'none' AND auth_secret_ref ~ '^(vault|k8s|aws-sm|gcp-sm|file|env)://')
    ),
  CONSTRAINT source_iceberg_catalog_uri_scheme
    CHECK (catalog_uri ~ '^(rest|hive|s3|file|gs|hdfs)://'),
  CONSTRAINT source_iceberg_valid_order
    CHECK (valid_to >= valid_from)
);

COMMENT ON TABLE eh_control.source_iceberg
  IS 'Apache Iceberg connector config. 1:1 with sources where kind=''iceberg''.';

-- ============================================================================
-- 4. source_duckdb
-- ============================================================================

CREATE TABLE eh_control.source_duckdb (
  source_id      UUID         NOT NULL,
  database_path  TEXT         NOT NULL DEFAULT ':memory:',
  extensions     TEXT[]       NOT NULL DEFAULT '{}',

  valid_from     TIMESTAMPTZ  NOT NULL DEFAULT now(),
  valid_to       TIMESTAMPTZ  NOT NULL DEFAULT 'infinity',
  is_current     BOOLEAN      NOT NULL DEFAULT true,

  PRIMARY KEY (source_id),
  CONSTRAINT source_duckdb_source_fk
    FOREIGN KEY (source_id) REFERENCES eh_control.sources(id),
  CONSTRAINT source_duckdb_valid_order
    CHECK (valid_to >= valid_from)
);

COMMENT ON TABLE eh_control.source_duckdb
  IS 'DuckDB connector config (in-memory or file-backed). 1:1 with sources where kind=''duckdb''.';

-- ============================================================================
-- 5. source_rag (forward-looking)
-- ============================================================================

CREATE TABLE eh_control.source_rag (
  source_id          UUID         NOT NULL,
  vector_store_uri   TEXT         NOT NULL,
  embedding_model    TEXT         NOT NULL,
  top_k_default      INT          NOT NULL DEFAULT 8,
  auth_secret_ref    TEXT,

  valid_from         TIMESTAMPTZ  NOT NULL DEFAULT now(),
  valid_to           TIMESTAMPTZ  NOT NULL DEFAULT 'infinity',
  is_current         BOOLEAN      NOT NULL DEFAULT true,

  PRIMARY KEY (source_id),
  CONSTRAINT source_rag_source_fk
    FOREIGN KEY (source_id) REFERENCES eh_control.sources(id),
  CONSTRAINT source_rag_top_k_pos
    CHECK (top_k_default > 0),
  CONSTRAINT source_rag_secret_ref_scheme
    CHECK (auth_secret_ref IS NULL OR auth_secret_ref ~ '^(vault|k8s|aws-sm|gcp-sm|file|env)://'),
  CONSTRAINT source_rag_valid_order
    CHECK (valid_to >= valid_from)
);

COMMENT ON TABLE eh_control.source_rag
  IS 'RAG / vector-store connector config (forward-looking, Phase 13+).';

-- ============================================================================
-- 6. source_model (forward-looking)
-- ============================================================================

CREATE TABLE eh_control.source_model (
  source_id            UUID         NOT NULL,
  provider             TEXT         NOT NULL,
  model_id             TEXT         NOT NULL,
  api_key_secret_ref   TEXT         NOT NULL,
  max_tokens_default   INT          NOT NULL DEFAULT 1024,

  valid_from           TIMESTAMPTZ  NOT NULL DEFAULT now(),
  valid_to             TIMESTAMPTZ  NOT NULL DEFAULT 'infinity',
  is_current           BOOLEAN      NOT NULL DEFAULT true,

  PRIMARY KEY (source_id),
  CONSTRAINT source_model_source_fk
    FOREIGN KEY (source_id) REFERENCES eh_control.sources(id),
  CONSTRAINT source_model_provider_chk
    CHECK (provider IN ('anthropic','openai','mistral','local')),
  CONSTRAINT source_model_max_tokens_pos
    CHECK (max_tokens_default > 0),
  CONSTRAINT source_model_secret_ref_scheme
    CHECK (api_key_secret_ref ~ '^(vault|k8s|aws-sm|gcp-sm|file|env)://'),
  CONSTRAINT source_model_valid_order
    CHECK (valid_to >= valid_from)
);

COMMENT ON TABLE eh_control.source_model
  IS 'Model / LLM connector config (forward-looking, Phase 13+).';

-- ============================================================================
-- 7. source_file (forward-looking)
-- ============================================================================

CREATE TABLE eh_control.source_file (
  source_id           UUID         NOT NULL,
  root_path           TEXT         NOT NULL,
  format              TEXT         NOT NULL,
  partition_pattern   TEXT,

  valid_from          TIMESTAMPTZ  NOT NULL DEFAULT now(),
  valid_to            TIMESTAMPTZ  NOT NULL DEFAULT 'infinity',
  is_current          BOOLEAN      NOT NULL DEFAULT true,

  PRIMARY KEY (source_id),
  CONSTRAINT source_file_source_fk
    FOREIGN KEY (source_id) REFERENCES eh_control.sources(id),
  CONSTRAINT source_file_format_chk
    CHECK (format IN ('parquet','csv','json','jsonl','arrow_ipc')),
  CONSTRAINT source_file_root_path_scheme
    CHECK (root_path ~ '^(file|s3|gs|hdfs|azure)://'),
  CONSTRAINT source_file_valid_order
    CHECK (valid_to >= valid_from)
);

COMMENT ON TABLE eh_control.source_file
  IS 'File-tree connector config (Parquet/CSV/JSON over object storage or local).';

-- ============================================================================
-- 8. source_snowflake (Phase 12, V1.1)
-- ============================================================================

CREATE TABLE eh_control.source_snowflake (
  source_id         UUID         NOT NULL,
  account           TEXT         NOT NULL,
  warehouse         TEXT         NOT NULL,
  database_name     TEXT         NOT NULL,
  schema_name       TEXT         NOT NULL,
  auth_kind         TEXT         NOT NULL DEFAULT 'key_pair',
  auth_secret_ref   TEXT         NOT NULL,

  valid_from        TIMESTAMPTZ  NOT NULL DEFAULT now(),
  valid_to          TIMESTAMPTZ  NOT NULL DEFAULT 'infinity',
  is_current        BOOLEAN      NOT NULL DEFAULT true,

  PRIMARY KEY (source_id),
  CONSTRAINT source_snowflake_source_fk
    FOREIGN KEY (source_id) REFERENCES eh_control.sources(id),
  CONSTRAINT source_snowflake_auth_chk
    CHECK (auth_kind IN ('password','key_pair','oauth')),
  CONSTRAINT source_snowflake_secret_ref_scheme
    CHECK (auth_secret_ref ~ '^(vault|k8s|aws-sm|gcp-sm|file|env)://'),
  CONSTRAINT source_snowflake_valid_order
    CHECK (valid_to >= valid_from)
);

COMMENT ON TABLE eh_control.source_snowflake
  IS 'Snowflake connector config (ADBC-backed, Phase 12 V1.1).';

-- ============================================================================
-- 9. source_mssql (Phase 12, V1.1)
-- ============================================================================

CREATE TABLE eh_control.source_mssql (
  source_id            UUID         NOT NULL,
  host                 TEXT         NOT NULL,
  port                 INT          NOT NULL DEFAULT 1433,
  instance             TEXT,
  database_name        TEXT         NOT NULL,
  auth_kind            TEXT         NOT NULL DEFAULT 'sql',
  username_secret_ref  TEXT,
  password_secret_ref  TEXT,

  valid_from           TIMESTAMPTZ  NOT NULL DEFAULT now(),
  valid_to             TIMESTAMPTZ  NOT NULL DEFAULT 'infinity',
  is_current           BOOLEAN      NOT NULL DEFAULT true,

  PRIMARY KEY (source_id),
  CONSTRAINT source_mssql_source_fk
    FOREIGN KEY (source_id) REFERENCES eh_control.sources(id),
  CONSTRAINT source_mssql_port_range
    CHECK (port BETWEEN 1 AND 65535),
  CONSTRAINT source_mssql_auth_chk
    CHECK (auth_kind IN ('sql','integrated','aad')),
  CONSTRAINT source_mssql_secret_match
    CHECK (
      (auth_kind = 'integrated' AND username_secret_ref IS NULL AND password_secret_ref IS NULL)
      OR
      (auth_kind <> 'integrated'
         AND username_secret_ref ~ '^(vault|k8s|aws-sm|gcp-sm|file|env)://'
         AND password_secret_ref ~ '^(vault|k8s|aws-sm|gcp-sm|file|env)://')
    ),
  CONSTRAINT source_mssql_valid_order
    CHECK (valid_to >= valid_from)
);

COMMENT ON TABLE eh_control.source_mssql
  IS 'SQL Server connector config (tiberius-backed, Phase 12 V1.1).';
