#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: scripts/pinned-product-deps.sh [--rev <git-rev>] [--url <git-url>]

Print exact-revision Cargo dependency entries for Chairman and Airline consumer
canaries. If --rev is omitted, HEAD must exist. If --url is omitted, the
origin remote is used when present; otherwise a local file:// URL is emitted for
local release-candidate testing.
USAGE
}

rev=""
url=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --rev)
      rev="${2:-}"
      shift 2
      ;;
    --url)
      url="${2:-}"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      usage >&2
      exit 2
      ;;
  esac
done

repo_root="$(git rev-parse --show-toplevel)"
cd "$repo_root"

if [[ -z "$rev" ]]; then
  if ! rev="$(git rev-parse --verify HEAD 2>/dev/null)"; then
    echo "error: world-infra has no HEAD commit; create a release-candidate commit before pinning products" >&2
    exit 1
  fi
else
  rev="$(git rev-parse --verify "$rev^{commit}")"
fi

if [[ -z "$url" ]]; then
  url="$(git config --get remote.origin.url || true)"
  if [[ -z "$url" ]]; then
    url="file://$(pwd -P)"
  fi
fi

cat <<EOF
# Exact world-infra revision for product canaries:
#   url = $url
#   rev = $rev

# Chairman [workspace.dependencies]
delivery-core = { git = "$url", rev = "$rev" }
http-primitives = { git = "$url", rev = "$rev" }
idempotency-core = { git = "$url", rev = "$rev" }
rate-limit-core = { git = "$url", rev = "$rev", features = ["redis"] }
tenant-scope-sqlx = { git = "$url", rev = "$rev" }
world-clock-core = { git = "$url", rev = "$rev" }
world-env = { git = "$url", rev = "$rev" }
world-identity-core = { git = "$url", rev = "$rev" }
world-telemetry = { git = "$url", rev = "$rev", features = ["otlp-http"] }
world-test-lite = { git = "$url", rev = "$rev" }

# Airline apps/airline-utils dependencies
world-env = { git = "$url", rev = "$rev" }

# Airline apps/airline-utils dev-dependencies
world-test-lite = { git = "$url", rev = "$rev" }

# Airline apps/loco-app dependencies
delivery-core = { git = "$url", rev = "$rev" }
http-primitives = { git = "$url", rev = "$rev" }
rate-limit-core = { git = "$url", rev = "$rev", features = ["redis"] }
tenant-scope-sqlx = { git = "$url", rev = "$rev", default-features = false, features = ["sqlx-postgres"] }
world-clock-core = { git = "$url", rev = "$rev" }
world-telemetry = { git = "$url", rev = "$rev", features = ["otlp-grpc-tonic"] }

# Airline apps/sim-engine dependencies
tenant-scope-sqlx = { git = "$url", rev = "$rev", default-features = false, features = ["sqlx-postgres"] }
EOF
