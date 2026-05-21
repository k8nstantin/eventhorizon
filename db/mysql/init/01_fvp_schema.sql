-- ============================================================================
-- EventHorizon — Phase 1 FVP MySQL schema
-- ============================================================================
-- Applied at MySQL 8.0 container startup via docker-compose init scripts.
-- Source of truth for the FVP data-source side.
-- ----------------------------------------------------------------------------
-- Conventions enforced here (architecture §5.1, zero-trust §10/§13/§14):
--   * UUIDv7 primary keys stored as BINARY(16) — app generates, no UUID_TO_BIN
--     shim in application code.
--   * INSERT-only writes — eh_service grant excludes UPDATE / DELETE / DDL.
--   * SCD Type 2 triad on every state-bearing table (valid_from, valid_to,
--     is_current) — set at insert, NEVER mutated.
--   * Account separation: eh_admin (operator-only) and eh_service (app,
--     SELECT-only for FVP; widened to SELECT+INSERT when append lands).
-- ============================================================================

-- 0. Database
-- ----------------------------------------------------------------------------

CREATE DATABASE IF NOT EXISTS eh_demo
  DEFAULT CHARACTER SET utf8mb4
  DEFAULT COLLATE utf8mb4_0900_as_cs;

USE eh_demo;

-- 1. customers (Phase 1 FVP test entity backing table)
-- ----------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS customers (
  id            BINARY(16)     NOT NULL,
  name          VARCHAR(255)   NOT NULL,
  email         VARCHAR(255)   NOT NULL,
  signup_at     TIMESTAMP(6)   NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
  ltv_usd       DECIMAL(12,2)  NOT NULL DEFAULT 0.00,

  -- SCD Type 2 triad
  valid_from    TIMESTAMP(6)   NOT NULL DEFAULT CURRENT_TIMESTAMP(6),
  valid_to      TIMESTAMP(6)   NOT NULL DEFAULT '9999-12-31 23:59:59.999999',
  is_current    TINYINT(1)     NOT NULL DEFAULT 1,

  PRIMARY KEY (id),
  INDEX        idx_email      (email),
  INDEX        idx_signup_at  (signup_at),
  INDEX        idx_chain      (id, valid_from),

  CONSTRAINT chk_ltv_nonneg     CHECK (ltv_usd >= 0),
  CONSTRAINT chk_is_current_bin CHECK (is_current IN (0, 1)),
  CONSTRAINT chk_valid_order    CHECK (valid_to >= valid_from)
) ENGINE=InnoDB ROW_FORMAT=DYNAMIC;

-- 2. Service accounts
-- ----------------------------------------------------------------------------
-- The passwords below are PLACEHOLDERS. In docker-compose, the init script
-- runs with operator-supplied env vars; the real init script template uses
-- ${MYSQL_EH_ADMIN_PASSWORD} and ${MYSQL_EH_SERVICE_PASSWORD} substitutions
-- so that no real secret lands in source.
-- ----------------------------------------------------------------------------

CREATE USER IF NOT EXISTS 'eh_admin'@'%'
  IDENTIFIED BY '__REPLACE_AT_INIT_eh_admin_password__';

GRANT ALL PRIVILEGES ON eh_demo.* TO 'eh_admin'@'%';

CREATE USER IF NOT EXISTS 'eh_service'@'%'
  IDENTIFIED BY '__REPLACE_AT_INIT_eh_service_password__';

-- Phase 1 FVP: SELECT only on the demo table.
-- When the append/update path lands in a later phase, the operator extends
-- these grants per zero-trust §11 (per-change authorisation).
GRANT SELECT ON eh_demo.customers TO 'eh_service'@'%';

-- Explicit denial check (MySQL's grant model is allow-list; absence is denial).
-- These are documented here so the contract is visible:
--   * eh_service has NO INSERT, NO UPDATE, NO DELETE, NO DDL on eh_demo.
--   * eh_service has NO privileges on any other schema.

FLUSH PRIVILEGES;

-- 3. Seeded test rows
-- ----------------------------------------------------------------------------
-- 10 rows with hand-rolled time-ordered UUIDv7 prefixes (timestamp ms || rand).
-- These specific bytes ARE deterministic across container startups so smoke
-- tests can reference customer IDs by literal value.
-- ----------------------------------------------------------------------------

INSERT INTO customers (id, name, email, signup_at, ltv_usd) VALUES
  (UNHEX(REPLACE('01914a01-7001-7001-8001-000000000001','-','')), 'Alice Example',     'alice@example.com',     '2024-09-01 09:00:00', 1250.00),
  (UNHEX(REPLACE('01914a01-7001-7001-8001-000000000002','-','')), 'Bob Example',       'bob@example.com',       '2024-10-12 14:30:00',  430.00),
  (UNHEX(REPLACE('01914a01-7001-7001-8001-000000000003','-','')), 'Carol Example',     'carol@example.com',     '2024-11-21 11:15:00', 2100.00),
  (UNHEX(REPLACE('01914a01-7001-7001-8001-000000000004','-','')), 'Dan Example',       'dan@example.com',       '2025-01-04 17:42:00',   25.00),
  (UNHEX(REPLACE('01914a01-7001-7001-8001-000000000005','-','')), 'Eve Example',       'eve@example.com',       '2025-02-19 08:05:00',  860.00),
  (UNHEX(REPLACE('01914a01-7001-7001-8001-000000000006','-','')), 'Frank Example',     'frank@example.com',     '2025-03-23 19:50:00',    0.00),
  (UNHEX(REPLACE('01914a01-7001-7001-8001-000000000007','-','')), 'Grace Example',     'grace@example.com',     '2025-04-30 13:00:00', 5400.00),
  (UNHEX(REPLACE('01914a01-7001-7001-8001-000000000008','-','')), 'Hugo Example',      'hugo@example.com',      '2025-08-14 10:20:00',  180.00),
  (UNHEX(REPLACE('01914a01-7001-7001-8001-000000000009','-','')), 'Iris Example',      'iris@example.com',      '2025-11-02 16:10:00', 3200.00),
  (UNHEX(REPLACE('01914a01-7001-7001-8001-00000000000a','-','')), 'Jules Example',     'jules@example.com',     '2026-02-08 12:45:00',  790.00);
