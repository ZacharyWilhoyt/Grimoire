#!/bin/sh
# Never let SQLx's dotenv fallback silently select the application's database.
set -eu
: "${DATABASE_URL:?Set DATABASE_URL explicitly to a disposable PostgreSQL test database}"
python3 - <<'PY'
import os
import sys
from urllib.parse import urlsplit, unquote
try:
    url = urlsplit(os.environ['DATABASE_URL'])
    database = unquote(url.path.removeprefix('/'))
except ValueError:
    sys.exit('Invalid test database URL.')
if url.scheme not in ('postgres', 'postgresql') or not database.endswith(('_test', '_tests')):
    sys.exit('Refusing integration tests: the explicit PostgreSQL database name must end in _test or _tests. Use a dedicated test role; never a production database.')
PY
cd "$(dirname "$0")/.."
exec cargo test --locked --manifest-path backend/Cargo.toml "$@" -- --ignored
