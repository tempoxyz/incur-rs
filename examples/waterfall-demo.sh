#!/usr/bin/env bash
# Demonstrates the config waterfall for each CLI option.
#
# Precedence (highest wins):
#   4. CLI arguments
#   3. Environment variables (DEPLOY_* prefix or explicit env)
#   2. TOML config file (deploy.toml)
#   1. Code defaults (#[incur(default = "...")])
#
# Usage: ./examples/waterfall-demo.sh

set -euo pipefail
cd "$(dirname "$0")/.."

BOLD='\033[1m'
DIM='\033[2m'
CYAN='\033[36m'
GREEN='\033[32m'
YELLOW='\033[33m'
MAGENTA='\033[35m'
RESET='\033[0m'

DEPLOY_TOML="deploy.toml"

header() { printf "\n${BOLD}${CYAN}═══ %s ═══${RESET}\n" "$1"; }
layer()  { printf "${DIM}  %-12s${RESET} %s\n" "$1" "$2"; }
sep()    { printf "${DIM}  ──────────────────────────────────────${RESET}\n"; }

run() {
    printf "${GREEN}  \$${RESET} %s\n" "$1"
    eval "$1" 2>&1 | sed 's/^/  /'
}

cleanup() { rm -f "$DEPLOY_TOML"; }
trap cleanup EXIT

# Build once
printf "${DIM}Building example...${RESET}\n"
cargo build --example derive --quiet 2>/dev/null

# ─────────────────────────────────────────────────────
header "Layer 1: Code defaults only"
layer "timeout" "default = \"30\"  (from #[incur(default)])"
layer "force"   "default = false  (bool zero-value)"
layer "api_key" "default = None   (Option<String>)"
sep
cleanup
run "./target/debug/examples/derive production"

# ─────────────────────────────────────────────────────
header "Layer 2: TOML config overrides code defaults"
cat > "$DEPLOY_TOML" <<'TOML'
timeout = 60
force = true
TOML
layer "file"    "$DEPLOY_TOML"
layer "timeout" "60  (was 30 from code default)"
layer "force"   "true  (was false from code default)"
sep
printf "${YELLOW}  deploy.toml:${RESET}\n"
sed 's/^/    /' "$DEPLOY_TOML"
sep
run "./target/debug/examples/derive production"

# ─────────────────────────────────────────────────────
header "Layer 3a: Env vars (prefix) override TOML"
layer "prefix"  "DEPLOY_*"
layer "timeout" "DEPLOY_TIMEOUT=90  (was 60 from TOML)"
layer "force"   "still true from TOML  (no env override)"
sep
run "DEPLOY_TIMEOUT=90 ./target/debug/examples/derive production"

# ─────────────────────────────────────────────────────
header "Layer 3b: Explicit env var (Opt::env)"
layer "env"     "MY_API_KEY=sk-secret-123"
layer "api_key" "sk-secret-123  (was not set)"
sep
run "MY_API_KEY=sk-secret-123 ./target/debug/examples/derive production"

# ─────────────────────────────────────────────────────
header "Layer 4: CLI args override everything"
layer "timeout" "--timeout 120  (was 90 from env, 60 from TOML, 30 from code)"
layer "force"   "--force not passed → falls back to env/TOML/code"
layer "api_key" "MY_API_KEY still set from env"
sep
run "DEPLOY_TIMEOUT=90 MY_API_KEY=sk-secret-123 ./target/debug/examples/derive production --timeout 120"

# ─────────────────────────────────────────────────────
header "Full waterfall summary"
printf "\n"
printf "  ${BOLD}Option     Code Default   TOML          Env Var            CLI Arg${RESET}\n"
printf "  ${DIM}─────────  ────────────   ──────────    ─────────────────  ─────────────${RESET}\n"
printf "  timeout    30             60            DEPLOY_TIMEOUT=90  --timeout 120\n"
printf "  force      false          true          (not set)          --force\n"
printf "  api_key    (not set)      (not set)     MY_API_KEY=sk-...  --api-key ...\n"
printf "\n"
printf "  ${BOLD}Winner: rightmost column that has a value.${RESET}\n\n"
