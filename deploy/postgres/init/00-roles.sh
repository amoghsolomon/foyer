#!/bin/bash
set -euo pipefail

psql -v ON_ERROR_STOP=1 \
  --username "$POSTGRES_USER" \
  --dbname "$POSTGRES_DB" \
  --set=source_database="$POSTGRES_DB" \
  --set=replication_password="$POWERSYNC_REPLICATION_PASSWORD" \
  --set=storage_password="$POWERSYNC_STORAGE_PASSWORD" <<'SQL'
SELECT format(
  'CREATE ROLE powersync_role WITH REPLICATION LOGIN PASSWORD %L',
  :'replication_password'
)
WHERE NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'powersync_role') \gexec
ALTER ROLE powersync_role WITH REPLICATION LOGIN PASSWORD :'replication_password';
SELECT format('GRANT CONNECT ON DATABASE %I TO powersync_role', :'source_database') \gexec
GRANT USAGE ON SCHEMA public TO powersync_role;
GRANT SELECT ON ALL TABLES IN SCHEMA public TO powersync_role;
ALTER DEFAULT PRIVILEGES IN SCHEMA public GRANT SELECT ON TABLES TO powersync_role;

SELECT format(
  'CREATE ROLE powersync_storage WITH LOGIN PASSWORD %L',
  :'storage_password'
)
WHERE NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'powersync_storage') \gexec
ALTER ROLE powersync_storage WITH LOGIN PASSWORD :'storage_password';
SQL

psql -v ON_ERROR_STOP=1 \
  --username "$POSTGRES_USER" \
  --dbname postgres \
  --set=storage_database=powersync <<'SQL'
SELECT format('CREATE DATABASE %I OWNER powersync_storage', :'storage_database')
WHERE NOT EXISTS (SELECT FROM pg_database WHERE datname = :'storage_database') \gexec
SELECT format('ALTER DATABASE %I OWNER TO powersync_storage', :'storage_database') \gexec
SELECT format('GRANT CREATE ON DATABASE %I TO powersync_storage', :'storage_database') \gexec
SQL
