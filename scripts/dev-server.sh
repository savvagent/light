#!/usr/bin/env bash
# Start the local PostgreSQL cluster (idempotent — never wipes existing data)
# and run the light-factory axum server against it.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(dirname "$SCRIPT_DIR")"

PGDATA="${PGDATA:-$ROOT/.pgdata}"
PGPORT="${PGPORT:-5432}"
PGHOST="127.0.0.1"
PGUSER="light"
PGDATABASE="light"
PGSOCKET_DIR="${PGSOCKET_DIR:-/tmp/opencode}"
PGLOG="$PGSOCKET_DIR/pg.log"

mkdir -p "$PGSOCKET_DIR"

# Initialize the cluster only if it doesn't exist. Never re-run initdb over an
# existing data dir — that would destroy previous data.
if [ ! -s "$PGDATA/PG_VERSION" ]; then
  echo "Initializing PostgreSQL cluster at $PGDATA …"
  initdb -U "$PGUSER" --auth=trust --encoding=UTF8 --locale=C.UTF-8 -D "$PGDATA"
fi

# Start the cluster if it isn't already running.
if ! pg_ctl -D "$PGDATA" status >/dev/null 2>&1; then
  echo "Starting PostgreSQL on $PGHOST:$PGPORT …"
  pg_ctl -D "$PGDATA" \
    -o "-p $PGPORT -k $PGSOCKET_DIR -c listen_addresses=$PGHOST" \
    -l "$PGLOG" start
fi

# Create the application database if it doesn't exist yet.
if [ "$(psql -h "$PGHOST" -p "$PGPORT" -U "$PGUSER" -d postgres -tAc \
      "SELECT 1 FROM pg_database WHERE datname='$PGDATABASE'")" != "1" ]; then
  echo "Creating database $PGDATABASE …"
  createdb -h "$PGHOST" -p "$PGPORT" -U "$PGUSER" "$PGDATABASE"
fi

export DATABASE_URL="postgres://$PGUSER@$PGHOST:$PGPORT/$PGDATABASE"

cd "$ROOT"
exec cargo run -p light-factory-server
