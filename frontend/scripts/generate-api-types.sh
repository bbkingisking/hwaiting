#!/bin/bash
# Regenerates src/lib/api-generated/ from the backend's OpenAPI spec.
#
# The spec itself (backend/openapi.json) is a build artifact, not checked in -
# regenerated fresh each run via `--print-openapi`, which the backend
# recognizes as a request to dump its spec to stdout and exit before
# touching the DB or any other config. The generated TS types *are*
# checked in, so a plain `deno install` works without a Rust toolchain; run
# this script and commit the diff whenever the backend's API contract changes.
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
FRONTEND_DIR="$SCRIPT_DIR/.."
BACKEND_DIR="$FRONTEND_DIR/../backend"
SPEC_PATH="$BACKEND_DIR/openapi.json"

echo "==> Building backend (if needed) and exporting OpenAPI spec..."
(cd "$BACKEND_DIR" && cargo run --quiet -- --print-openapi > "$SPEC_PATH")

echo "==> Generating TypeScript types from $SPEC_PATH..."
(cd "$FRONTEND_DIR" && deno task generate-api-types:from-spec --input "$SPEC_PATH")

echo "==> Done. Diff src/lib/api-generated/ and commit if the contract changed."
