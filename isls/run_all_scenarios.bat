@echo off
:: run_all_scenarios.bat — ISLS full validation suite (Windows)
:: Runs all 5 synthetic scenarios end-to-end and generates a combined HTML report.
SETLOCAL ENABLEDELAYEDEXPANSION

set ISLS_HOME=%USERPROFILE%\.isls
set RESULTS_DIR=%ISLS_HOME%\results
set ISLS=cargo run --bin isls --release --

:: ── Step 0: full initial clean + build ────────────────────────────────────────
echo [SETUP] Cleaning all ISLS state...
if exist "%ISLS_HOME%" rmdir /s /q "%ISLS_HOME%"
mkdir "%RESULTS_DIR%"

echo [BUILD] Building release binary (this may take a moment)...
cargo build --bin isls --release 2>nul
echo [BUILD] Done.
echo.

:: ── Scenario arrays ───────────────────────────────────────────────────────────
set SCENARIOS[0]=S-Basic
set SCENARIOS[1]=S-Regime
set SCENARIOS[2]=S-Causal
set SCENARIOS[3]=S-Break
set SCENARIOS[4]=S-Scale

set TICKS[0]=100
set TICKS[1]=200
set TICKS[2]=200
set TICKS[3]=600
set TICKS[4]=200

:: ── Steps 1-5: run each scenario ─────────────────────────────────────────────
for /L %%i in (0,1,4) do (
    set /A N=%%i+1
    set S=!SCENARIOS[%%i]!
    set T=!TICKS[%%i]!

    echo [!N!/5] !S!: cleaning...
    if exist "%ISLS_HOME%\data"     rmdir /s /q "%ISLS_HOME%\data"
    if exist "%ISLS_HOME%\metrics"  rmdir /s /q "%ISLS_HOME%\metrics"
    if exist "%ISLS_HOME%\reports"  rmdir /s /q "%ISLS_HOME%\reports"
    if exist "%ISLS_HOME%\replay"   rmdir /s /q "%ISLS_HOME%\replay"
    if exist "%ISLS_HOME%\config.json" del /q "%ISLS_HOME%\config.json"
    if not exist "%RESULTS_DIR%" mkdir "%RESULTS_DIR%"

    echo [!N!/5] !S!: ingesting...
    %ISLS% ingest --adapter synthetic --scenario !S! 2>nul

    echo [!N!/5] !S!: running !T! ticks...
    %ISLS% run --mode shadow --ticks !T! 2>nul

    echo [!N!/5] !S!: validating...
    %ISLS% validate --formal 2>nul > "%RESULTS_DIR%\!S!-validate.txt"

    :: Extract pass rate for the progress summary
    set PASS_RATE=unknown
    for /f "tokens=3" %%p in ('findstr /C:"Pass rate:" "%RESULTS_DIR%\!S!-validate.txt" 2^>nul') do set PASS_RATE=%%p
    set CRYSTALS=0
    for /f "tokens=2" %%c in ('findstr /C:"  Total:" "%RESULTS_DIR%\!S!-validate.txt" 2^>nul') do set CRYSTALS=%%c
    echo [!N!/5] !S!: validating... !CRYSTALS! crystals, !PASS_RATE! pass

    :: Save per-scenario JSON files that full-html will read
    %ISLS% report --json 2>nul > "%RESULTS_DIR%\!S!-metrics.json"
    if exist "%ISLS_HOME%\reports\latest-formal.json" (
        copy /y "%ISLS_HOME%\reports\latest-formal.json" "%RESULTS_DIR%\!S!-formal.json" >nul
    )

    :: Combine validate text + metrics JSON into the spec-required results.txt
    type "%RESULTS_DIR%\!S!-validate.txt"  > "%RESULTS_DIR%\!S!-results.txt"
    echo.                                  >> "%RESULTS_DIR%\!S!-results.txt"
    type "%RESULTS_DIR%\!S!-metrics.json"  >> "%RESULTS_DIR%\!S!-results.txt"
)

echo.

:: ── Step 3: benchmarks ────────────────────────────────────────────────────────
echo [BENCH] Running benchmarks...
%ISLS% bench 2>nul

echo.

:: ── Step 4: generate combined HTML report ─────────────────────────────────────
echo [REPORT] Generating full HTML report...
for /f "delims=" %%p in ('%ISLS% report full-html 2^>nul') do set REPORT_PATH=%%p

echo.
echo Done: %REPORT_PATH%

ENDLOCAL
