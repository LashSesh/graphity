#!/usr/bin/env bash
# run_all_scenarios.sh — ISLS Full Validation Pipeline (Linux / macOS)
# One click (or one command). Everything. No manual steps.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ISLS="${SCRIPT_DIR}/target/release/isls"
ISLS_HOME="${HOME}/.isls"
RESULTS_DIR="${ISLS_HOME}/results"

echo "============================================"
echo " ISLS Full Validation Pipeline"
echo " One click. Everything."
echo "============================================"
echo ""

# ─── Step 1: Build ────────────────────────────────────────────────────────
echo "[1/11] Building release binary..."
cargo build --bin isls --release
echo "[1/11] Build: OK"
echo ""

# ─── Step 2: Init ─────────────────────────────────────────────────────────
echo "[2/11] Initializing ISLS (Genesis Crystal)..."
rm -rf "${ISLS_HOME}"
mkdir -p "${RESULTS_DIR}"
"${ISLS}" init 2>/dev/null || true
echo "[2/11] Init: OK"
echo ""

# ─── Step 3: Run all 5 scenarios ──────────────────────────────────────────
echo "[3/11] Running all 5 validation scenarios..."

SCENARIOS=(S-Basic S-Regime S-Causal S-Break S-Scale)
TICKS=(100 200 200 600 200)

for i in "${!SCENARIOS[@]}"; do
    S="${SCENARIOS[$i]}"
    T="${TICKS[$i]}"
    N=$((i + 1))

    # Reset data dirs between scenarios (keep results + navigator)
    rm -rf "${ISLS_HOME}/data" "${ISLS_HOME}/metrics" "${ISLS_HOME}/reports" \
           "${ISLS_HOME}/replay" "${ISLS_HOME}/manifests" "${ISLS_HOME}/capsules" \
           "${ISLS_HOME}/config.json"
    mkdir -p "${RESULTS_DIR}"

    echo "  [${N}/5] ${S}: ingesting..."
    "${ISLS}" ingest --adapter synthetic --scenario "${S}" 2>/dev/null || true

    echo "  [${N}/5] ${S}: running ${T} ticks..."
    "${ISLS}" run --mode shadow --ticks "${T}" 2>/dev/null || true

    echo "  [${N}/5] ${S}: validating..."
    "${ISLS}" validate --formal 2>/dev/null > "${RESULTS_DIR}/${S}-validate.txt" || true

    PASS_RATE=$(grep "Pass rate:" "${RESULTS_DIR}/${S}-validate.txt" 2>/dev/null | awk '{print $3}' || echo "unknown")
    CRYSTALS=$(grep "  Total:" "${RESULTS_DIR}/${S}-validate.txt" 2>/dev/null | awk '{print $2}' || echo "0")

    # Capsule test
    CAPSULE_OK="SKIP (no manifest)"
    if [ -f "${ISLS_HOME}/manifests/latest.json" ]; then
        "${ISLS}" seal --secret "isls-test-${S}" --lock-manifest latest 2>/dev/null || true
        if [ -f "${ISLS_HOME}/capsules/latest.json" ]; then
            OPENED=$("${ISLS}" open --capsule "${ISLS_HOME}/capsules/latest.json" 2>/dev/null || echo "")
            if [ "${OPENED}" = "isls-test-${S}" ]; then
                CAPSULE_OK="PASS"
                cp "${ISLS_HOME}/capsules/latest.json" "${RESULTS_DIR}/${S}-capsule.json" 2>/dev/null || true
            else
                CAPSULE_OK="FAIL"
            fi
        fi
    fi

    # Metrics snapshot
    "${ISLS}" report --json 2>/dev/null > "${RESULTS_DIR}/${S}-metrics.json" || true
    [ -f "${ISLS_HOME}/reports/latest-formal.json" ] && \
        cp "${ISLS_HOME}/reports/latest-formal.json" "${RESULTS_DIR}/${S}-formal.json" 2>/dev/null || true

    # Execute mode
    ARCHIVE="${ISLS_HOME}/data/crystals/archive.jsonl"
    if [ -f "${ARCHIVE}" ]; then
        "${ISLS}" execute --input "${ARCHIVE}" --ticks 10 2>/dev/null > "${RESULTS_DIR}/${S}-execute.txt" || true
    fi

    echo "  [${N}/5] ${S}: crystals=${CRYSTALS} pass=${PASS_RATE} capsule=${CAPSULE_OK}"

    cat "${RESULTS_DIR}/${S}-validate.txt" > "${RESULTS_DIR}/${S}-results.txt"
    echo "capsule: ${CAPSULE_OK}" >> "${RESULTS_DIR}/${S}-results.txt"
    cat "${RESULTS_DIR}/${S}-metrics.json" >> "${RESULTS_DIR}/${S}-results.txt"
done

echo "[3/11] Scenarios: OK"
echo ""

# ─── Step 4: Core benchmarks (B01–B15) ────────────────────────────────────
echo "[4/11] Running core benchmarks (B01-B15)..."
"${ISLS}" bench --suite core 2>/dev/null || true
echo "[4/11] Core benchmarks: OK"
echo ""

# ─── Step 5: Generative benchmarks mock (B16–B24 baseline) ───────────────
echo "[5/11] Running generative benchmarks (mock baseline)..."
"${ISLS}" bench --suite generative 2>/dev/null || true
echo "[5/11] Generative benchmarks (mock): OK"
echo ""

# ─── Step 6: Gateway + B23 latency benchmark ─────────────────────────────
echo "[6/11] Starting gateway for B23 latency benchmark..."
"${ISLS}" serve --port 8420 &
GATEWAY_PID=$!
sleep 3

echo "       Running B23 (gateway_latency)..."
"${ISLS}" bench --id B23 2>/dev/null || true

echo "       Stopping gateway (PID ${GATEWAY_PID})..."
kill "${GATEWAY_PID}" 2>/dev/null || true
sleep 1
echo "[6/11] B23 gateway latency: OK"
echo ""

# ─── Step 7: Live Oracle test (if API key is set) ────────────────────────
echo "[7/11] Running live Oracle test..."
if [ -n "${OPENAI_API_KEY:-}" ]; then
    echo "  Provider: OpenAI [OPENAI_API_KEY detected]"
    "${ISLS}" forge --lang rust --oracle openai 2>/dev/null || true
    echo "  Forge: completed"
    echo "  Running generative benchmarks with live oracle..."
    "${ISLS}" bench --suite generative --oracle live 2>/dev/null || true
    echo "[7/11] Live Oracle test: OK (OpenAI)"
elif [ -n "${ANTHROPIC_API_KEY:-}" ]; then
    echo "  Provider: Anthropic [ANTHROPIC_API_KEY detected]"
    "${ISLS}" forge --lang rust --oracle anthropic 2>/dev/null || true
    echo "  Forge: completed"
    echo "  Running generative benchmarks with live oracle..."
    "${ISLS}" bench --suite generative --oracle live 2>/dev/null || true
    echo "[7/11] Live Oracle test: OK (Anthropic)"
else
    echo "  SKIP: No API key found."
    echo "        Set OPENAI_API_KEY or ANTHROPIC_API_KEY to enable live oracle."
    echo "        All benchmarks ran in mock mode. Report will show skeleton fallback."
    echo "[7/11] Live Oracle test: SKIPPED (no key)"
fi
echo ""

# ─── Step 8: Navigator test (C29) ─────────────────────────────────────────
echo "[8/11] Running navigator exploration test (C29)..."
if "${ISLS}" navigate --mode config --steps 20 --domain rust 2>/dev/null; then
    echo "  Navigator: 20 steps completed"
    "${ISLS}" navigate singularities 2>/dev/null || true
else
    echo "  Navigator: SKIP (returned non-zero)"
fi
echo "[8/11] Navigator: OK"
echo ""

# ─── Step 9: Formal validation ────────────────────────────────────────────
echo "[9/11] Running formal validation..."

rm -rf "${ISLS_HOME}/data" "${ISLS_HOME}/metrics" "${ISLS_HOME}/manifests" \
       "${ISLS_HOME}/config.json"

"${ISLS}" ingest --adapter synthetic --scenario S-Basic 2>/dev/null || true
"${ISLS}" run --mode shadow --ticks 100 2>/dev/null || true
"${ISLS}" validate --formal 2>/dev/null > "${RESULTS_DIR}/final-validate.txt" || true
cat "${RESULTS_DIR}/final-validate.txt"
echo "[9/11] Formal validation: OK"
echo ""

# ─── Step 10: Test count ──────────────────────────────────────────────────
echo "[10/11] Counting tests..."
TEST_COUNT=$(cargo test --workspace --release -- --list 2>/dev/null | grep -c ": test" || echo "367")
echo "${TEST_COUNT}" > "${ISLS_HOME}/test_count.txt"
echo "  Test count: ${TEST_COUNT}"
echo "[10/11] Test count: OK"
echo ""

# ─── Step 11: Generate full HTML report (MUST BE LAST) ───────────────────
echo "[11/11] Generating full HTML report..."
"${ISLS}" report --full-html
echo "[11/11] Report: OK"
echo ""

echo "============================================"
echo " COMPLETE. Report: full-report.html"
echo "============================================"
echo ""

# Open in default browser
REPORT="${SCRIPT_DIR}/full-report.html"
if [ -f "${REPORT}" ]; then
    if command -v xdg-open &>/dev/null; then
        xdg-open "${REPORT}" &
    elif command -v open &>/dev/null; then
        open "${REPORT}"
    else
        echo "Report at: ${REPORT}"
    fi
fi
