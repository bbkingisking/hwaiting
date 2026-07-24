#!/bin/bash
# Regenerates src/lib/api-schema.d.ts from the backend's OpenAPI spec.
#
# The spec itself (backend/openapi.json) is a build artifact, not checked in -
# regenerated fresh each run via `PRINT_OPENAPI=1`, which the backend
# recognizes as a request to dump its spec to stdout and exit before
# touching the DB or any other config. The generated TS types file *is*
# checked in, so a plain `npm install` works without a Rust toolchain; run
# this script and commit the diff whenever the backend's API contract changes.
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
FRONTEND_DIR="$SCRIPT_DIR/.."
BACKEND_DIR="$FRONTEND_DIR/../backend"
SPEC_PATH="$BACKEND_DIR/openapi.json"

echo "==> Building backend (if needed) and exporting OpenAPI spec..."
(cd "$BACKEND_DIR" && PRINT_OPENAPI=1 cargo run --quiet > "$SPEC_PATH")

echo "==> Generating TypeScript types from $SPEC_PATH..."
(cd "$FRONTEND_DIR" && npx openapi-typescript "$SPEC_PATH" -o src/lib/api-schema.d.ts)

echo "==> Done. Diff src/lib/api-schema.d.ts and commit if the contract changed."
