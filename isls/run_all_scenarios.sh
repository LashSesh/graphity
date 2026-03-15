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

    # Combine validate text + metrics JSON into the spec-required results.txt
    {
        cat "${RESULTS_DIR}/${S}-validate.txt"
        echo ""
        cat "${RESULTS_DIR}/${S}-metrics.json"
    } > "${RESULTS_DIR}/${S}-results.txt"
done

echo ""

# ── Step 3: benchmarks ────────────────────────────────────────────────────────
echo "[BENCH] Running benchmarks..."
isls bench

echo ""

# ── Step 4: generate combined HTML report ─────────────────────────────────────
echo "[REPORT] Generating full HTML report..."
REPORT_PATH=$(isls report full-html)

echo ""
echo "Done: ${REPORT_PATH}"
