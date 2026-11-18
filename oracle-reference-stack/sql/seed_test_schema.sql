-- Seed script for the Oracle Forms testbed (Oracle Database Free, PDB FREEPDB1).
-- Run as SYSTEM. Local-development defaults only: change the passwords for anything beyond a throwaway container.
-- Usage (from the host, Ubuntu WSL or macOS):
--   (printf 'DEFINE db_user=%s\nDEFINE db_password=%s\n' "$DB_USER" "$DB_PASSWORD"; cat sql/seed_test_schema.sql) | podman exec -i oracle23ai-lite sqlplus -s system/$ORACLE_PWD@localhost/FREEPDB1

SET ECHO ON
SET FEEDBACK ON
WHENEVER SQLERROR CONTINUE

-- Schema owner and password are substitution variables (&db_user, &db_password), set from .env by the command above.
-- Recreate the user from scratch (ignore "user does not exist" on the first run)
DROP USER &db_user CASCADE;

WHENEVER SQLERROR EXIT SQL.SQLCODE

-- FREEPDB1 has no USERS tablespace; create a dedicated one for the test schema (ignore "already exists" on rerun)
WHENEVER SQLERROR CONTINUE
CREATE TABLESPACE forms_data DATAFILE '/opt/oracle/oradata/FREE/FREEPDB1/forms_data01.dbf' SIZE 100M AUTOEXTEND ON NEXT 25M;
WHENEVER SQLERROR EXIT SQL.SQLCODE

CREATE USER &db_user IDENTIFIED BY "&db_password" DEFAULT TABLESPACE forms_data QUOTA UNLIMITED ON forms_data;
GRANT CREATE SESSION, CREATE TABLE, CREATE SEQUENCE, CREATE VIEW, CREATE PROCEDURE TO &db_user;

-- Tables are created inside the schema owner's schema
CREATE TABLE &db_user..departments (
  dept_id    NUMBER(4)     CONSTRAINT departments_pk PRIMARY KEY,
  dept_name  VARCHAR2(60)  NOT NULL,
  location   VARCHAR2(60)
);

CREATE TABLE &db_user..employees (
  emp_id     NUMBER(6)     CONSTRAINT employees_pk PRIMARY KEY,
  first_name VARCHAR2(40)  NOT NULL,
  last_name  VARCHAR2(40)  NOT NULL,
  email      VARCHAR2(80)  UNIQUE,
  dept_id    NUMBER(4)     CONSTRAINT employees_dept_fk REFERENCES &db_user..departments (dept_id),
  hire_date  DATE          DEFAULT SYSDATE,
  salary     NUMBER(10,2)
);

CREATE TABLE &db_user..test_items (
  item_id    NUMBER(6)     CONSTRAINT test_items_pk PRIMARY KEY,
  item_name  VARCHAR2(60)  NOT NULL,
  qty        NUMBER(6)     DEFAULT 0,
  price      NUMBER(10,2),
  amount     NUMBER(10,2)   -- the column samples/test_db.fmb reads (SELECT item_name, TO_CHAR(amount) ...)
);

-- Sample records
INSERT INTO &db_user..departments VALUES (10, 'Engineering', 'Building A');
INSERT INTO &db_user..departments VALUES (20, 'Sales', 'Building B');
INSERT INTO &db_user..departments VALUES (30, 'Support', 'Remote');

INSERT INTO &db_user..employees VALUES (1001, 'Ada',   'Lovelace', 'ada@example.test',   10, DATE '2024-01-15', 125000);
INSERT INTO &db_user..employees VALUES (1002, 'Grace', 'Hopper',   'grace@example.test', 10, DATE '2023-06-01', 118000);
INSERT INTO &db_user..employees VALUES (1003, 'Linus', 'Torvalds', 'linus@example.test', 20, DATE '2025-02-10',  96000);
INSERT INTO &db_user..employees VALUES (1004, 'Mary',  'Shelley',  'mary@example.test',  30, DATE '2022-09-20',  72000);
INSERT INTO &db_user..employees VALUES (1005, 'Alan',  'Turing',   'alan@example.test', 10, DATE '2025-07-01', 131000);
INSERT INTO &db_user..employees VALUES (1006, 'Edsger','Dijkstra', 'edsger@example.test', 20, DATE '2024-11-11', 104000);

INSERT INTO &db_user..test_items (item_id, item_name, qty, price, amount) VALUES (1, 'Widget A',       25, 29.99, 29.99);
INSERT INTO &db_user..test_items (item_id, item_name, qty, price, amount) VALUES (2, 'Gadget B',        5, 99.50, 99.50);
INSERT INTO &db_user..test_items (item_id, item_name, qty, price, amount) VALUES (3, 'Super Gadget C', 12, 149.95, 149.95);
INSERT INTO &db_user..test_items (item_id, item_name, qty, price, amount) VALUES (4, 'Doohickey',       0, 3.75, 3.75);
INSERT INTO &db_user..test_items (item_id, item_name, qty, price, amount) VALUES (5, 'Thingamajig',    40, 2.10, 2.10);

COMMIT;

-- Sample form support objects (all sample forms in samples/)
CREATE TABLE &db_user..assignments (
  agent_ref  VARCHAR2(30)  CONSTRAINT assignments_pk PRIMARY KEY,
  agent_name VARCHAR2(100) NOT NULL
);

CREATE TABLE &db_user..claims (
  claim_id     NUMBER(10)    CONSTRAINT claims_pk PRIMARY KEY,
  policy_number VARCHAR2(30) NOT NULL,
  claim_date   DATE          DEFAULT SYSDATE,
  claim_amount NUMBER(12,2),
  status       VARCHAR2(20),
  description  VARCHAR2(255),
  owner        VARCHAR2(30),
  reference    VARCHAR2(50)
);

CREATE TABLE &db_user..customers (
  customer_id   NUMBER(10)    CONSTRAINT customers_pk PRIMARY KEY,
  customer_name VARCHAR2(100) NOT NULL,
  city          VARCHAR2(50)
);

CREATE TABLE &db_user..orders (
  order_id      NUMBER(10)    CONSTRAINT orders_pk PRIMARY KEY,
  customer_name VARCHAR2(100) NOT NULL,
  order_date    DATE          DEFAULT SYSDATE,
  status        VARCHAR2(20),
  total_amount  NUMBER(12,2)
);

CREATE TABLE &db_user..order_lines (
  line_id    NUMBER(10)    CONSTRAINT order_lines_pk PRIMARY KEY,
  order_id   NUMBER(10)    CONSTRAINT order_lines_order_fk REFERENCES &db_user..orders (order_id),
  product    VARCHAR2(100) NOT NULL,
  quantity   NUMBER(6),
  unit_price NUMBER(10,2),
  cancelled  VARCHAR2(1)   DEFAULT 'N'
);

CREATE TABLE &db_user..stock (
  id  NUMBER(10) CONSTRAINT stock_pk PRIMARY KEY,
  qty NUMBER(10) DEFAULT 0
);

CREATE TABLE &db_user..customer_invoices (
  invoice_id  NUMBER(10)   CONSTRAINT customer_invoices_pk PRIMARY KEY,
  customer_id NUMBER(10)   NOT NULL,
  invoice_amt NUMBER(12,2) NOT NULL,
  status      VARCHAR2(20) NOT NULL
);

CREATE TABLE &db_user..account_audit_log (
  audit_id    NUMBER GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  customer_id NUMBER(10),
  action_name VARCHAR2(60),
  created_at  DATE DEFAULT SYSDATE
);

CREATE OR REPLACE PROCEDURE &db_user..log_account_audit(
  p_customer_id IN NUMBER,
  p_action      IN VARCHAR2
) IS
BEGIN
  INSERT INTO &db_user..account_audit_log (customer_id, action_name)
  VALUES (p_customer_id, p_action);
END;
/

INSERT INTO &db_user..assignments VALUES ('AG-101', 'Ada Lovelace');
INSERT INTO &db_user..assignments VALUES ('AG-102', 'Grace Hopper');

INSERT INTO &db_user..claims (claim_id, policy_number, claim_date, claim_amount, status, description, owner, reference)
VALUES (1, 'POL-1001', DATE '2026-01-10', 1250.00, 'OPEN', 'Initial sample claim', USER, 'REF-001');

INSERT INTO &db_user..customers VALUES (1, 'Acme Corp', 'Victoria');
INSERT INTO &db_user..customers VALUES (2, 'Pacific Traders', 'Vancouver');

INSERT INTO &db_user..orders VALUES (1001, 'Acme Corp', DATE '2026-02-01', 'OPEN', 299.90);
INSERT INTO &db_user..order_lines VALUES (1, 1001, 'Widget A', 10, 29.99, 'N');

INSERT INTO &db_user..stock VALUES (1, 50);

INSERT INTO &db_user..customer_invoices VALUES (5001, 1, 450.00, 'UNPAID');
COMMIT;

-- Queries
SET LINESIZE 200
SET PAGESIZE 100
COLUMN dept_name FORMAT A16
COLUMN full_name FORMAT A22
COLUMN item_name FORMAT A14

SELECT 'departments' AS table_name, COUNT(*) AS row_count FROM &db_user..departments
UNION ALL SELECT 'employees', COUNT(*) FROM &db_user..employees
UNION ALL SELECT 'test_items', COUNT(*) FROM &db_user..test_items;

SELECT e.emp_id, e.first_name || ' ' || e.last_name AS full_name, d.dept_name, e.salary
FROM &db_user..employees e
JOIN &db_user..departments d ON d.dept_id = e.dept_id
ORDER BY e.salary DESC;

SELECT d.dept_name, COUNT(e.emp_id) AS headcount, ROUND(AVG(e.salary), 2) AS avg_salary
FROM &db_user..departments d
LEFT JOIN &db_user..employees e ON e.dept_id = d.dept_id
GROUP BY d.dept_name
ORDER BY d.dept_name;

SELECT item_id, item_name, qty, price, qty * price AS stock_value
FROM &db_user..test_items
WHERE qty > 0
ORDER BY stock_value DESC;

EXIT
