@echo off
:: run_all_scenarios.bat — ISLS full validation suite (Windows)
:: Runs all 5 synthetic scenarios end-to-end and generates a combined HTML report.
SETLOCAL ENABLEDELAYEDEXPANSION

set ISLS_HOME=%USERPROFILE%\.isls
set RESULTS_DIR=%ISLS_HOME%\results

:: Determine repo root relative to this script
set "SCRIPT_DIR=%~dp0"
set "BINARY=%SCRIPT_DIR%target\release\isls.exe"

:: ── Step 0: full initial clean + build ────────────────────────────────────────
echo [SETUP] Cleaning all ISLS state...
if exist "%ISLS_HOME%" rmdir /s /q "%ISLS_HOME%"
mkdir "%RESULTS_DIR%"

echo [BUILD] Building release binary...
echo         (First build may be slow: rusqlite compiles bundled SQLite from C source.
echo          Requires MSVC Build Tools or MinGW-w64. If this fails, install one of:
echo          - Visual Studio Build Tools: https://aka.ms/vs/17/release/vs_BuildTools.exe
echo          - LLVM+clang via winget:     winget install LLVM.LLVM
echo          - MinGW-w64 via rustup:      rustup toolchain install stable-x86_64-pc-windows-gnu)
echo.

cargo build --bin isls --release
if errorlevel 1 (
    echo.
    echo [ERROR] Build failed. See output above.
    echo         Common fix: install Visual Studio Build Tools and rerun.
    exit /b 1
)
echo [BUILD] Done.
echo.

set ISLS="%BINARY%"

echo [INIT] Establishing system constitution (Genesis Crystal)...
%ISLS% init 2>nul
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
    if exist "%ISLS_HOME%\data"      rmdir /s /q "%ISLS_HOME%\data"
    if exist "%ISLS_HOME%\metrics"   rmdir /s /q "%ISLS_HOME%\metrics"
    if exist "%ISLS_HOME%\reports"   rmdir /s /q "%ISLS_HOME%\reports"
    if exist "%ISLS_HOME%\replay"    rmdir /s /q "%ISLS_HOME%\replay"
    if exist "%ISLS_HOME%\manifests" rmdir /s /q "%ISLS_HOME%\manifests"
    if exist "%ISLS_HOME%\capsules"  rmdir /s /q "%ISLS_HOME%\capsules"
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

    :: ── Extension: capsule seal/open ─────────────────────────────────────────
    :: Run BEFORE execute so the manifest is definitively the one from `isls run`.
    :: If execute were to overwrite latest.json with a different run_id, seal and
    :: open would disagree, causing a false FAIL.
    echo [!N!/5] !S!: capsule seal/open test...
    set CAPSULE_OK=SKIP (no manifest)
    if exist "%ISLS_HOME%\manifests\latest.json" (
        %ISLS% seal --secret "isls-test-secret-!S!" --lock-manifest latest 2>nul
        if exist "%ISLS_HOME%\capsules\latest.json" (
            set OPENED_SECRET=
            for /f "delims=" %%o in ('%ISLS% open --capsule "%ISLS_HOME%\capsules\latest.json" 2^>nul') do set OPENED_SECRET=%%o
            if "!OPENED_SECRET!"=="isls-test-secret-!S!" (
                set CAPSULE_OK=PASS
                copy /y "%ISLS_HOME%\capsules\latest.json" "%RESULTS_DIR%\!S!-capsule.json" >nul
            ) else if "!OPENED_SECRET!"=="" (
                set CAPSULE_OK=FAIL (open produced no output)
            ) else (
                set CAPSULE_OK=FAIL (open returned: !OPENED_SECRET!)
            )
        ) else (
            set CAPSULE_OK=SKIP (seal produced no capsule)
        )
    )
    echo   capsule: !CAPSULE_OK!

    :: ── Extension: execute mode + manifest ───────────────────────────────────
    echo [!N!/5] !S!: execute mode + manifest...
    set MANIFEST_ID=N/A
    set ARCHIVE_PATH=%ISLS_HOME%\data\crystals\archive.jsonl
    if exist "!ARCHIVE_PATH!" (
        %ISLS% execute --input "!ARCHIVE_PATH!" --ticks 10 2>nul > "%RESULTS_DIR%\!S!-execute.txt"
        if exist "%ISLS_HOME%\manifests\latest.json" (
            copy /y "%ISLS_HOME%\manifests\latest.json" "%RESULTS_DIR%\!S!-manifest.json" >nul
            set MANIFEST_ID=generated
        )
    )
    echo   manifest_id: !MANIFEST_ID!

    :: Combine validate + manifest info + metrics into results.txt
    type "%RESULTS_DIR%\!S!-validate.txt"  > "%RESULTS_DIR%\!S!-results.txt"
    echo.                                  >> "%RESULTS_DIR%\!S!-results.txt"
    echo manifest_id: !MANIFEST_ID!        >> "%RESULTS_DIR%\!S!-results.txt"
    echo capsule_test: !CAPSULE_OK!        >> "%RESULTS_DIR%\!S!-results.txt"
    echo.                                  >> "%RESULTS_DIR%\!S!-results.txt"
    type "%RESULTS_DIR%\!S!-metrics.json"  >> "%RESULTS_DIR%\!S!-results.txt"
)

echo.

:: ── Execute mode integration test ─────────────────────────────────────────────
echo [EXECUTE] Running execute-mode integration test (S-Basic crystal)...
if exist "%ISLS_HOME%\data"      rmdir /s /q "%ISLS_HOME%\data"
if exist "%ISLS_HOME%\metrics"   rmdir /s /q "%ISLS_HOME%\metrics"
if exist "%ISLS_HOME%\reports"   rmdir /s /q "%ISLS_HOME%\reports"
if exist "%ISLS_HOME%\manifests" rmdir /s /q "%ISLS_HOME%\manifests"
if exist "%ISLS_HOME%\capsules"  rmdir /s /q "%ISLS_HOME%\capsules"
if exist "%ISLS_HOME%\config.json" del /q "%ISLS_HOME%\config.json"

%ISLS% ingest --adapter synthetic --scenario S-Basic 2>nul
%ISLS% run --mode shadow --ticks 100 2>nul

:: ── Capsule integration test ──────────────────────────────────────────────────
:: Run BEFORE execute so the manifest is definitively the one from `isls run`.
echo [CAPSULE] Running capsule integration test...
set CAPSULE_RESULT=SKIP (no manifest)
if exist "%ISLS_HOME%\manifests\latest.json" (
    %ISLS% seal --secret "isls-test-secret" --lock-manifest latest 2>nul
    if exist "%ISLS_HOME%\capsules\latest.json" (
        set OPENED_FINAL=
        for /f "delims=" %%o in ('%ISLS% open --capsule "%ISLS_HOME%\capsules\latest.json" 2^>nul') do set OPENED_FINAL=%%o
        if "!OPENED_FINAL!"=="isls-test-secret" (
            set CAPSULE_RESULT=PASS
        ) else if "!OPENED_FINAL!"=="" (
            set CAPSULE_RESULT=FAIL (open produced no output)
        ) else (
            set CAPSULE_RESULT=FAIL (open returned: !OPENED_FINAL!)
        )
    ) else (
        set CAPSULE_RESULT=SKIP (seal produced no capsule)
    )
)
echo   capsule: %CAPSULE_RESULT%
echo CAPSULE_INTEGRATION: %CAPSULE_RESULT% > "%RESULTS_DIR%\capsule-integration.txt"

set ARCHIVE_PATH=%ISLS_HOME%\data\crystals\archive.jsonl
if exist "%ARCHIVE_PATH%" (
    %ISLS% execute --input "%ARCHIVE_PATH%" --ticks 10 2>nul > "%RESULTS_DIR%\execute-integration.txt"
    %ISLS% validate --formal 2>nul >> "%RESULTS_DIR%\execute-integration.txt"
)
echo   execute-mode integration: done

echo.

:: ── Step 3: benchmarks ────────────────────────────────────────────────────────
echo [BENCH] Running benchmarks B01-B15...
%ISLS% bench 2>nul

echo.

:: ── Step 4: generate combined HTML report ─────────────────────────────────────
echo [REPORT] Generating full HTML report...
for /f "delims=" %%p in ('%ISLS% report full-html 2^>nul') do set REPORT_PATH=%%p

echo.
echo Done: %REPORT_PATH%

ENDLOCAL
