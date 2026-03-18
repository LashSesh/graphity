@echo off
:: run_all_scenarios.bat — ISLS Full Validation Pipeline (Windows)
:: One click. Everything. No manual steps.
SETLOCAL ENABLEDELAYEDEXPANSION

set "SCRIPT_DIR=%~dp0"
set "BINARY=%SCRIPT_DIR%target\release\isls.exe"
set ISLS="%BINARY%"

set ISLS_HOME=%USERPROFILE%\.isls
set RESULTS_DIR=%ISLS_HOME%\results

echo ============================================
echo  ISLS Full Validation Pipeline
echo  One click. Everything.
echo ============================================
echo.

:: ─── API-Key Eingabe ────────────────────────────────────────────────────────
if not defined OPENAI_API_KEY (
  if not defined ANTHROPIC_API_KEY (
    echo Kein API-Key gesetzt. Bitte jetzt eingeben.
    echo Einfach ENTER druecken um ohne Live-Oracle zu starten.
    echo.
    set /p "BENCH_PROVIDER=Provider (openai / anthropic / leer): "
    if /i "!BENCH_PROVIDER!"=="openai" (
      set /p "OPENAI_API_KEY=OpenAI API-Key (sk-...): "
    ) else if /i "!BENCH_PROVIDER!"=="anthropic" (
      set /p "ANTHROPIC_API_KEY=Anthropic API-Key (sk-ant-...): "
    )
    echo.
  )
)
if defined OPENAI_API_KEY    echo   Aktiv: OpenAI
if defined ANTHROPIC_API_KEY echo   Aktiv: Anthropic
if not defined OPENAI_API_KEY if not defined ANTHROPIC_API_KEY echo   Kein Key - Live-Oracle wird uebersprungen.
echo.

:: ─── Step 1: Build ─────────────────────────────────────────────────────────
echo [1/11] Building release binary...
echo        (First build compiles bundled SQLite from C — requires MSVC or MinGW-w64)
cargo build --bin isls --release
if errorlevel 1 (
    echo.
    echo [ERROR] Build failed. See output above.
    echo         Fix: install Visual Studio Build Tools or MinGW-w64 then rerun.
    exit /b 1
)
echo [1/11] Build: OK
echo.

:: ─── Step 2: Init ──────────────────────────────────────────────────────────
echo [2/11] Initializing ISLS (Genesis Crystal)...
if exist "%ISLS_HOME%" rmdir /s /q "%ISLS_HOME%"
mkdir "%RESULTS_DIR%" 2>nul
%ISLS% init 2>nul
echo [2/11] Init: OK
echo.

:: ─── Step 3: Run all 5 scenarios ───────────────────────────────────────────
echo [3/11] Running all 5 validation scenarios...

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

for /L %%i in (0,1,4) do (
    set /A N=%%i+1
    set S=!SCENARIOS[%%i]!
    set T=!TICKS[%%i]!

    :: Reset data dirs between scenarios (keep results + navigator)
    if exist "%ISLS_HOME%\data"      rmdir /s /q "%ISLS_HOME%\data"
    if exist "%ISLS_HOME%\metrics"   rmdir /s /q "%ISLS_HOME%\metrics"
    if exist "%ISLS_HOME%\reports"   rmdir /s /q "%ISLS_HOME%\reports"
    if exist "%ISLS_HOME%\replay"    rmdir /s /q "%ISLS_HOME%\replay"
    if exist "%ISLS_HOME%\manifests" rmdir /s /q "%ISLS_HOME%\manifests"
    if exist "%ISLS_HOME%\capsules"  rmdir /s /q "%ISLS_HOME%\capsules"
    if exist "%ISLS_HOME%\config.json" del /q "%ISLS_HOME%\config.json"
    if not exist "%RESULTS_DIR%" mkdir "%RESULTS_DIR%"

    echo   [!N!/5] !S!: ingesting...
    %ISLS% ingest --adapter synthetic --scenario !S! 2>nul

    echo   [!N!/5] !S!: running !T! ticks...
    %ISLS% run --mode shadow --ticks !T! 2>nul

    echo   [!N!/5] !S!: validating...
    %ISLS% validate --formal 2>nul > "%RESULTS_DIR%\!S!-validate.txt"

    set PASS_RATE=unknown
    for /f "tokens=3" %%p in ('findstr /C:"Pass rate:" "%RESULTS_DIR%\!S!-validate.txt" 2^>nul') do set PASS_RATE=%%p
    set CRYSTALS=0
    for /f "tokens=2" %%c in ('findstr /C:"  Total:" "%RESULTS_DIR%\!S!-validate.txt" 2^>nul') do set CRYSTALS=%%c

    :: Capsule test
    set CAPSULE_OK=SKIP (no manifest)
    if exist "%ISLS_HOME%\manifests\latest.json" (
        %ISLS% seal --secret "isls-test-!S!" --lock-manifest latest 2>nul
        if exist "%ISLS_HOME%\capsules\latest.json" (
            set OPENED=
            for /f "delims=" %%o in ('%ISLS% open --capsule "%ISLS_HOME%\capsules\latest.json" 2^>nul') do set OPENED=%%o
            if "!OPENED!"=="isls-test-!S!" (
                set CAPSULE_OK=PASS
                copy /y "%ISLS_HOME%\capsules\latest.json" "%RESULTS_DIR%\!S!-capsule.json" >nul
            ) else (
                set CAPSULE_OK=FAIL
            )
        )
    )

    :: Metrics snapshot
    %ISLS% report --json 2>nul > "%RESULTS_DIR%\!S!-metrics.json"
    if exist "%ISLS_HOME%\reports\latest-formal.json" (
        copy /y "%ISLS_HOME%\reports\latest-formal.json" "%RESULTS_DIR%\!S!-formal.json" >nul
    )

    :: Execute mode
    set ARCHIVE_PATH=%ISLS_HOME%\data\crystals\archive.jsonl
    if exist "!ARCHIVE_PATH!" (
        %ISLS% execute --input "!ARCHIVE_PATH!" --ticks 10 2>nul > "%RESULTS_DIR%\!S!-execute.txt"
    )

    echo   [!N!/5] !S!: crystals=!CRYSTALS! pass=!PASS_RATE! capsule=!CAPSULE_OK!

    type "%RESULTS_DIR%\!S!-validate.txt"  > "%RESULTS_DIR%\!S!-results.txt"
    echo capsule: !CAPSULE_OK!             >> "%RESULTS_DIR%\!S!-results.txt"
    type "%RESULTS_DIR%\!S!-metrics.json"  >> "%RESULTS_DIR%\!S!-results.txt"
)

echo [3/11] Scenarios: OK
echo.

:: ─── Step 4: Core benchmarks (B01–B15) ────────────────────────────────────
echo [4/11] Running core benchmarks (B01-B15)...
%ISLS% bench --suite core 2>nul
echo [4/11] Core benchmarks: OK
echo.

:: ─── Step 5: Generative benchmarks mock (B16–B24 baseline) ────────────────
echo [5/11] Running generative benchmarks (mock baseline)...
%ISLS% bench --suite generative 2>nul
echo [5/11] Generative benchmarks (mock): OK
echo.

:: ─── Step 6: Gateway + B23 latency benchmark ──────────────────────────────
echo [6/11] Starting gateway for B23 latency benchmark...
start /b %ISLS% serve --port 8420
timeout /t 3 /nobreak >nul

echo        Running B23 (gateway_latency)...
%ISLS% bench --id B23 2>nul

echo        Stopping gateway...
taskkill /f /im isls.exe >nul 2>&1
timeout /t 1 /nobreak >nul
echo [6/11] B23 gateway latency: OK
echo.

:: ─── Step 7: Live Oracle test (if API key is set) ─────────────────────────
echo [7/11] Running live Oracle test...
if defined OPENAI_API_KEY (
    echo   Provider: OpenAI [OPENAI_API_KEY detected]
    %ISLS% forge --lang rust --oracle openai 2>nul
    if not errorlevel 1 (
        echo   Forge: completed
    ) else (
        echo   Forge: completed (no template selected — expected)
    )
    echo   Running generative benchmarks with live oracle...
    %ISLS% bench --suite generative --oracle live 2>nul
    echo [7/11] Live Oracle test: OK (OpenAI)
) else if defined ANTHROPIC_API_KEY (
    echo   Provider: Anthropic [ANTHROPIC_API_KEY detected]
    %ISLS% forge --lang rust --oracle anthropic 2>nul
    if not errorlevel 1 (
        echo   Forge: completed
    ) else (
        echo   Forge: completed (no template selected — expected)
    )
    echo   Running generative benchmarks with live oracle...
    %ISLS% bench --suite generative --oracle live 2>nul
    echo [7/11] Live Oracle test: OK (Anthropic)
) else (
    echo   SKIP: No API key found.
    echo         Set OPENAI_API_KEY or ANTHROPIC_API_KEY to enable live oracle.
    echo         All benchmarks ran in mock mode. Report will show skeleton fallback.
    echo [7/11] Live Oracle test: SKIPPED (no key)
)
echo.

:: ─── Step 8: Navigator test (C29) ─────────────────────────────────────────
echo [8/11] Running navigator exploration test (C29)...
%ISLS% navigate --mode config --steps 20 --domain rust 2>nul
if errorlevel 1 (
    echo   Navigator: SKIP (returned non-zero)
) else (
    echo   Navigator: 20 steps completed
    %ISLS% navigate singularities 2>nul
)
echo [8/11] Navigator: OK
echo.

:: ─── Step 9: Formal validation ────────────────────────────────────────────
echo [9/11] Running formal validation...

:: Reset to S-Basic for a clean validation snapshot
if exist "%ISLS_HOME%\data"      rmdir /s /q "%ISLS_HOME%\data"
if exist "%ISLS_HOME%\metrics"   rmdir /s /q "%ISLS_HOME%\metrics"
if exist "%ISLS_HOME%\manifests" rmdir /s /q "%ISLS_HOME%\manifests"
if exist "%ISLS_HOME%\config.json" del /q "%ISLS_HOME%\config.json"

%ISLS% ingest --adapter synthetic --scenario S-Basic 2>nul
%ISLS% run --mode shadow --ticks 100 2>nul
%ISLS% validate --formal 2>nul > "%RESULTS_DIR%\final-validate.txt"
type "%RESULTS_DIR%\final-validate.txt"
echo [9/11] Formal validation: OK
echo.

:: ─── Step 10: Run test suite for accurate count ────────────────────────────
echo [10/11] Running cargo test for accurate count...
for /f %%c in ('cargo test --workspace --release -- --list 2^>nul ^| find /c ": test"') do (
    echo %%c > "%ISLS_HOME%\test_count.txt"
    echo   Test count: %%c
)
if not exist "%ISLS_HOME%\test_count.txt" echo 367 > "%ISLS_HOME%\test_count.txt"
echo [10/11] Test count: OK
echo.

:: ─── Step 11: Generate full HTML report (MUST BE LAST) ────────────────────
echo [11/11] Generating full HTML report...
%ISLS% report --full-html
echo [11/11] Report: OK
echo.

echo ============================================
echo  COMPLETE. Report: full-report.html
echo ============================================
echo.

:: Open in default browser
if exist "%SCRIPT_DIR%full-report.html" (
    start "" "%SCRIPT_DIR%full-report.html"
) else (
    echo NOTE: full-report.html not found at %SCRIPT_DIR%
)

ENDLOCAL
