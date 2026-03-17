#!/usr/bin/env bash
# run_all_scenarios.sh — ISLS full validation suite (Linux/macOS)
# Runs all 5 synthetic scenarios end-to-end and generates a combined HTML report.
set -euo pipefail

ISLS_HOME="${HOME:-/root}/.isls"
RESULTS_DIR="${ISLS_HOME}/results"

# ── Helper: run the isls binary (suppressing cargo/rustc noise on stderr) ─────
isls() {
    cargo run --bin isls --release -- "$@" 2>/dev/null
}

# ── Helper: clean per-scenario state while preserving results/ ────────────────
clean_scenario_state() {
    rm -rf "${ISLS_HOME}/data" \
           "${ISLS_HOME}/metrics" \
           "${ISLS_HOME}/reports" \
           "${ISLS_HOME}/replay" \
           "${ISLS_HOME}/manifests" \
           "${ISLS_HOME}/capsules" \
           "${ISLS_HOME}/config.json"
    mkdir -p "${RESULTS_DIR}"
}

# ── Step 0: full initial clean + build ────────────────────────────────────────
echo "[SETUP] Cleaning all ISLS state..."
rm -rf "${ISLS_HOME}"
mkdir -p "${RESULTS_DIR}"

echo "[BUILD] Building release binary (this may take a moment)..."
cargo build --bin isls --release 2>/dev/null
echo "[BUILD] Done."
echo ""

echo "[INIT] Establishing system constitution (Genesis Crystal)..."
isls init
echo ""

# ── Scenario table: name  ticks ───────────────────────────────────────────────
declare -a SCENARIO_NAMES=("S-Basic" "S-Regime" "S-Causal" "S-Break" "S-Scale")
declare -a SCENARIO_TICKS=(100       200        200        600       200)

# ── Step 1-5: run each scenario ───────────────────────────────────────────────
for i in "${!SCENARIO_NAMES[@]}"; do
    N=$(( i + 1 ))
    S="${SCENARIO_NAMES[$i]}"
    T="${SCENARIO_TICKS[$i]}"

    echo "[${N}/5] ${S}: cleaning..."
    clean_scenario_state

    printf "[%d/5] %s: ingesting...\n" "${N}" "${S}"
    isls ingest --adapter synthetic --scenario "${S}"

    printf "[%d/5] %s: running %d ticks...\n" "${N}" "${S}" "${T}"
    isls run --mode shadow --ticks "${T}"

    printf "[%d/5] %s: validating... " "${N}" "${S}"
    isls validate --formal > "${RESULTS_DIR}/${S}-validate.txt"
    # Extract crystal count and pass rate for the progress line
    CRYSTALS=$(grep -E "^  Total:" "${RESULTS_DIR}/${S}-validate.txt" \
               | awk '{print $2}' || echo "0")
    PASS_RATE=$(grep -E "Pass rate:" "${RESULTS_DIR}/${S}-validate.txt" \
                | awk '{print $3}' || echo "?")
    printf "%s crystals, %s pass\n" "${CRYSTALS}" "${PASS_RATE}"

    # Save per-scenario JSON files that full-html will read
    isls report --json > "${RESULTS_DIR}/${S}-metrics.json"
    if [ -f "${ISLS_HOME}/reports/latest-formal.json" ]; then
        cp "${ISLS_HOME}/reports/latest-formal.json" "${RESULTS_DIR}/${S}-formal.json"
    fi

    # ── Extension: build + verify execution manifest ──────────────────────────
    printf "[%d/5] %s: building execution manifest...\n" "${N}" "${S}"
    ARCHIVE_PATH="${ISLS_HOME}/data/crystals/archive.jsonl"
    MANIFEST_ID="N/A"
    if [ -f "${ARCHIVE_PATH}" ]; then
        isls execute --input "${ARCHIVE_PATH}" --ticks 10 \
            > "${RESULTS_DIR}/${S}-execute.txt" 2>&1 || true
        if [ -f "${ISLS_HOME}/manifests/latest.json" ]; then
            MANIFEST_ID=$(python3 -c \
                "import json,sys; d=json.load(open('${ISLS_HOME}/manifests/latest.json')); \
                 h=bytes(d['run_id']); print(h.hex()[:16]+'...')" 2>/dev/null \
                || grep -o '"run_id":\[[^]]*\]' "${ISLS_HOME}/manifests/latest.json" \
                | head -1 | cut -c1-40 || echo "generated")
            cp "${ISLS_HOME}/manifests/latest.json" \
               "${RESULTS_DIR}/${S}-manifest.json" 2>/dev/null || true
        fi
    else
        echo "  (no archive yet — skipping execute)" \
            >> "${RESULTS_DIR}/${S}-validate.txt" || true
    fi
    printf "  manifest_id: %s\n" "${MANIFEST_ID}"

    # ── Extension: capsule seal/open round-trip ───────────────────────────────
    printf "[%d/5] %s: capsule seal/open test..." "${N}" "${S}"
    CAPSULE_OK="SKIP (no manifest)"
    if [ -f "${ISLS_HOME}/manifests/latest.json" ]; then
        isls seal --secret "isls-test-secret-${S}" \
            --lock-manifest latest 2>/dev/null || true
        if [ -f "${ISLS_HOME}/capsules/latest.json" ]; then
            OPENED=$(isls open \
                --capsule "${ISLS_HOME}/capsules/latest.json" 2>/dev/null || echo "FAIL")
            if [ "${OPENED}" = "isls-test-secret-${S}" ]; then
                CAPSULE_OK="PASS"
                cp "${ISLS_HOME}/capsules/latest.json" \
                   "${RESULTS_DIR}/${S}-capsule.json" 2>/dev/null || true
            else
                CAPSULE_OK="FAIL"
            fi
        fi
    fi
    printf " %s\n" "${CAPSULE_OK}"

    # Combine validate text + manifest + metrics JSON into results.txt
    {
        cat "${RESULTS_DIR}/${S}-validate.txt"
        echo ""
        echo "manifest_id: ${MANIFEST_ID}"
        echo "capsule_test: ${CAPSULE_OK}"
        echo ""
        cat "${RESULTS_DIR}/${S}-metrics.json"
    } > "${RESULTS_DIR}/${S}-results.txt"
done

echo ""

# ── Execute mode integration test (S-Basic crystal → execute → validate) ──────
echo "[EXECUTE] Running execute-mode integration test (S-Basic crystal)..."
clean_scenario_state
isls ingest --adapter synthetic --scenario S-Basic
isls run --mode shadow --ticks 100
EXECUTE_CRYSTALS=0
EXECUTE_PASS="SKIP"
ARCHIVE_PATH="${ISLS_HOME}/data/crystals/archive.jsonl"
if [ -f "${ARCHIVE_PATH}" ]; then
    isls execute --input "${ARCHIVE_PATH}" --ticks 10 \
        > "${RESULTS_DIR}/execute-integration.txt" 2>&1 || true
    isls validate --formal >> "${RESULTS_DIR}/execute-integration.txt" 2>&1 || true
    EXECUTE_CRYSTALS=$(grep -E "^  Total:" "${RESULTS_DIR}/execute-integration.txt" \
                       | tail -1 | awk '{print $2}' || echo "0")
    EXECUTE_PASS=$(grep -E "Pass rate:" "${RESULTS_DIR}/execute-integration.txt" \
                   | tail -1 | awk '{print $3}' || echo "N/A")
fi
printf "  execute-mode crystals: %s, pass: %s\n" "${EXECUTE_CRYSTALS}" "${EXECUTE_PASS}"

# ── Capsule integration test ──────────────────────────────────────────────────
echo "[CAPSULE] Running capsule integration test..."
CAPSULE_RESULT="SKIP (no manifest)"
if [ -f "${ISLS_HOME}/manifests/latest.json" ]; then
    isls seal --secret "isls-test-secret" --lock-manifest latest 2>/dev/null || true
    if [ -f "${ISLS_HOME}/capsules/latest.json" ]; then
        OPENED=$(isls open \
            --capsule "${ISLS_HOME}/capsules/latest.json" 2>/dev/null || echo "FAIL")
        if [ "${OPENED}" = "isls-test-secret" ]; then
            CAPSULE_RESULT="PASS"
        else
            CAPSULE_RESULT="FAIL (got: ${OPENED})"
        fi
    fi
fi
echo "  capsule: ${CAPSULE_RESULT}"
echo "CAPSULE_INTEGRATION: ${CAPSULE_RESULT}" > "${RESULTS_DIR}/capsule-integration.txt"

echo ""

# ── Step 3: benchmarks ────────────────────────────────────────────────────────
echo "[BENCH] Running benchmarks B01-B15..."
isls bench

echo ""

# ── Step 4: generate combined HTML report ─────────────────────────────────────
echo "[REPORT] Generating full HTML report..."
REPORT_PATH=$(isls report full-html)

echo ""
echo "Done: ${REPORT_PATH}"
