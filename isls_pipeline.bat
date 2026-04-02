@echo off
:: ═══════════════════════════════════════════════════════════════════════════
:: isls_pipeline.bat -- ISLS D6 Full Acceptance Pipeline
::
:: Doppelklick genuegt. Baut, testet, generiert 6 Apps via LLM,
:: verifiziert Kompilierung, prueft Norm-Emergenz, scrapet Projekte,
:: generiert ISLS Studio via forge-self, prueft Infra-Norms.
:: ═══════════════════════════════════════════════════════════════════════════
chcp 65001 >nul 2>nul
setlocal EnableDelayedExpansion

:: ─── ANSI-Farben ─────────────────────────────────────────────────────────
for /F "tokens=1,2 delims=#" %%a in ('"prompt #$E# & echo on & for %%b in (1) do rem"') do set "ESC=%%b"
set "GRN=!ESC![32m"
set "RED=!ESC![31m"
set "YLW=!ESC![33m"
set "CYN=!ESC![36m"
set "BLD=!ESC![1m"
set "RST=!ESC![0m"

:: ─── Pfade ───────────────────────────────────────────────────────────────
set "ROOT=%~dp0"
if "!ROOT:~-1!"=="\" set "ROOT=!ROOT:~0,-1!"

:: ─── Timestamp (locale-unabhaengig via wmic) ─────────────────────────────
for /f "skip=1 tokens=1" %%t in ('wmic os get localdatetime 2^>nul') do (
    set "DT=%%t"
    goto :GOT_DT
)
:GOT_DT
set "TS=!DT:~0,4!!DT:~4,2!!DT:~6,2!_!DT:~8,2!!DT:~10,2!"
set "TS_DISPLAY=!DT:~0,4!-!DT:~4,2!-!DT:~6,2! !DT:~8,2!:!DT:~10,2!"

set "LOG_DIR=!ROOT!\logs"
set "OUT_DIR=!ROOT!\output\pipeline_!TS!"
set "ERR_LOG=!LOG_DIR!\errors_!TS!.log"
set "REPORT_LOG=!LOG_DIR!\report_!TS!.log"
set "ISLS_BIN=!ROOT!\target\release\isls.exe"

mkdir "!LOG_DIR!" 2>nul
mkdir "!OUT_DIR!" 2>nul

:: ─── Zaehler ─────────────────────────────────────────────────────────────
set "PASS_COUNT=0"
set "FAIL_COUNT=0"
set "TEST_PASS=0"
set "TEST_FAIL=0"
set "TEST_WARNING="
set "BUILD_OK="
set "HAS_ERRORS="
set "CANDIDATES=0"
set "AUTO_NORMS=0"
set "NORMS_SIZE=n/a"

:: Domain-Ergebnisse (6 Domains)
set "RESULT_0=SKIP"
set "RESULT_1=SKIP"
set "RESULT_2=SKIP"
set "RESULT_3=SKIP"
set "RESULT_4=SKIP"
set "RESULT_5=SKIP"
set "ENTITIES_0=?"
set "ENTITIES_1=?"
set "ENTITIES_2=?"
set "ENTITIES_3=?"
set "ENTITIES_4=?"
set "ENTITIES_5=?"

:: D5 Scrape results
set "SCRAPE_PETSHOP=SKIP"
set "SCRAPE_PETSHOP_ARTIFACTS=?"
set "SCRAPE_SELF=SKIP"
set "SCRAPE_SELF_ARTIFACTS=?"

:: D6 Moebius results
set "STUDIO_GEN=SKIP"
set "STUDIO_COMPILE=SKIP"
set "STUDIO_SEED=SKIP"
set "INFRA_NORMS=0"

:: ═══════════════════════════════════════════════════════════════════════════
:: Header
:: ═══════════════════════════════════════════════════════════════════════════
echo.
echo !BLD!!CYN!======================================================!RST!
echo !BLD!!CYN!  ISLS D6 -- Full Acceptance Pipeline!RST!
echo !BLD!!CYN!  !TS_DISPLAY!!RST!
echo !BLD!!CYN!======================================================!RST!
echo.

:: ═══════════════════════════════════════════════════════════════════════════
:: Section 1: API-Key
:: ═══════════════════════════════════════════════════════════════════════════
echo !BLD![Step 1/8] API-Key!RST!
echo.

if defined OPENAI_API_KEY (
    set "_K=!OPENAI_API_KEY!"
    set "_FIRST=!_K:~0,8!"
    set "_LAST=!_K:~-4!"
    echo   Vorhandener Key: !_FIRST!...!_LAST!
    echo.
    set "_USE="
    set /p "_USE=  Enter = verwenden, oder neuen Key eingeben: "
    if "!_USE!" NEQ "" set "OPENAI_API_KEY=!_USE!"
    echo   !GRN!Key aktiv.!RST!
) else (
    set /p "OPENAI_API_KEY=  OpenAI API Key (sk-...): "
    if "!OPENAI_API_KEY!"=="" (
        echo.
        echo   !RED!Kein API-Key eingegeben. Abbruch.!RST!
        goto :ABORT
    )
    echo   !GRN!Key gesetzt.!RST!
)
echo.

:: ═══════════════════════════════════════════════════════════════════════════
:: Section 2: Workspace Build
:: ═══════════════════════════════════════════════════════════════════════════
echo !BLD![Step 2/8] Workspace Build!RST!
echo   cargo build --workspace --release ...
echo.

set "BUILD_LOG=!LOG_DIR!\build_!TS!.log"
cd /d "!ROOT!"
cargo build --workspace --release > "!BUILD_LOG!" 2>&1
if errorlevel 1 (
    set "BUILD_OK=FAIL"
    set "HAS_ERRORS=1"
    echo   !RED!BUILD FEHLGESCHLAGEN!RST!
    echo.
    echo   !YLW!Vollstaendiger Log: !BUILD_LOG!!RST!
    echo   !YLW!Kopiere den Log und gib ihn Claude.!RST!
    call :LOG_ERROR "workspace" "cargo build --workspace" "!ROOT!" "!BUILD_LOG!"
    goto :ABORT
)

set "BUILD_OK=PASS"
echo   !GRN!BUILD OK!RST!
echo.

:: ═══════════════════════════════════════════════════════════════════════════
:: Section 3: Workspace Tests
:: ═══════════════════════════════════════════════════════════════════════════
echo !BLD![Step 3/8] Workspace Tests!RST!
echo   cargo test --workspace ...
echo.

set "TEST_LOG=!LOG_DIR!\test_!TS!.log"
cd /d "!ROOT!"
cargo test --workspace > "!TEST_LOG!" 2>&1

:: Parse test results -- zaehle passed/failed
call :COUNT_TESTS "!TEST_LOG!"

if !TEST_FAIL! GTR 0 (
    set "TEST_WARNING=1"
    set "HAS_ERRORS=1"
    echo   !YLW!!TEST_PASS! passed, !TEST_FAIL! FAILED!RST!
    echo   !YLW!Log: !TEST_LOG!!RST!
    call :LOG_ERROR "workspace" "cargo test --workspace" "!ROOT!" "!TEST_LOG!"
) else (
    echo   !GRN!!TEST_PASS! passed, 0 failed!RST!
)
echo.

:: ═══════════════════════════════════════════════════════════════════════════
:: Section 4: D4 Acceptance -- 6 Domains generieren
:: ═══════════════════════════════════════════════════════════════════════════
echo !BLD![Step 4/8] D4 Acceptance Pipeline -- 6 Domains!RST!
echo.

:: Jede Domain via Subroutine (vermeidet tiefe Verschachtelung)
call :RUN_DOMAIN 0 "petshop" "Pet shop with animals, owners, and veterinary appointments"
call :RUN_DOMAIN 1 "hotel" "Hotel management with rooms, guests, bookings, and housekeeping tasks"
call :RUN_DOMAIN 2 "clinic" "Medical clinic with patients, doctors, appointments, and prescriptions"
call :RUN_DOMAIN 3 "gym" "Fitness gym with members, trainers, workout classes, and membership plans"
call :RUN_DOMAIN 4 "school" "School management with students, teachers, courses, and grades"
call :RUN_DOMAIN 5 "spa" "Spa and wellness center with treatments, therapists, bookings, and packages"
echo.

:: ═══════════════════════════════════════════════════════════════════════════
:: Section 5: D5 -- Scrape Validation
:: ═══════════════════════════════════════════════════════════════════════════
echo !BLD![Step 5/8] D5 Scrape Validation!RST!
echo.

:: --- Scrape petshop (generated project) ---
echo   !CYN![1/2]!RST! Scraping petshop output ...
set "_SCRAPE_LOG_1=!LOG_DIR!\scrape_petshop_!TS!.log"
cd /d "!ROOT!"
if exist "!OUT_DIR!\petshop" (
    "!ISLS_BIN!" scrape --path "!OUT_DIR!\petshop" > "!_SCRAPE_LOG_1!" 2>&1
    if errorlevel 1 (
        echo          !RED!FAIL!RST!
        set "SCRAPE_PETSHOP=FAIL"
        set "HAS_ERRORS=1"
        call :LOG_ERROR "scrape-petshop" "isls scrape --path petshop" "!ROOT!" "!_SCRAPE_LOG_1!"
    ) else (
        set "SCRAPE_PETSHOP=PASS"
        set "SCRAPE_PETSHOP_ARTIFACTS=0"
        for /f "usebackq delims=" %%L in ("!_SCRAPE_LOG_1!") do (
            echo %%L | findstr /c:"artifact" >nul 2>nul
            if not errorlevel 1 set "SCRAPE_PETSHOP_ARTIFACTS=%%L"
        )
        echo          !GRN!PASS!RST!
    )
) else (
    echo          !YLW!SKIP - petshop not generated!RST!
)

:: --- Scrape ISLS source code ---
echo   !CYN![2/2]!RST! Scraping ISLS source ...
set "_SCRAPE_LOG_2=!LOG_DIR!\scrape_self_!TS!.log"
cd /d "!ROOT!"
"!ISLS_BIN!" scrape --path "!ROOT!" --domain "isls-self" > "!_SCRAPE_LOG_2!" 2>&1
if errorlevel 1 (
    echo          !RED!FAIL!RST!
    set "SCRAPE_SELF=FAIL"
    set "HAS_ERRORS=1"
    call :LOG_ERROR "scrape-self" "isls scrape --path . --domain isls-self" "!ROOT!" "!_SCRAPE_LOG_2!"
) else (
    set "SCRAPE_SELF=PASS"
    set "SCRAPE_SELF_ARTIFACTS=0"
    for /f "usebackq delims=" %%L in ("!_SCRAPE_LOG_2!") do (
        echo %%L | findstr /c:"artifact" >nul 2>nul
        if not errorlevel 1 set "SCRAPE_SELF_ARTIFACTS=%%L"
    )
    echo          !GRN!PASS!RST!
)
echo.

:: ═══════════════════════════════════════════════════════════════════════════
:: Section 6: D6 -- Moebius (forge-self)
:: ═══════════════════════════════════════════════════════════════════════════
echo !BLD![Step 6/8] D6 Moebius -- forge-self!RST!
echo.

:: --- Generate ISLS Studio ---
echo   !CYN![1/3]!RST! Generating ISLS Studio ...
set "_STUDIO_LOG=!LOG_DIR!\forge_self_!TS!.log"
cd /d "!ROOT!"
"!ISLS_BIN!" forge-self --api-key !OPENAI_API_KEY! --output "!OUT_DIR!\isls-studio" > "!_STUDIO_LOG!" 2>&1
if errorlevel 1 (
    echo          !RED!Generation FAILED!RST!
    set "STUDIO_GEN=FAIL"
    set "HAS_ERRORS=1"
    call :LOG_ERROR "isls-studio" "isls forge-self" "!ROOT!" "!_STUDIO_LOG!"
    goto :SKIP_STUDIO_COMPILE
)
set "STUDIO_GEN=PASS"
echo          !GRN!PASS!RST!

:: --- Compile generated Studio ---
echo   !CYN![2/3]!RST! Compiling ISLS Studio backend ...
set "_STUDIO_COMPILE_LOG=!LOG_DIR!\compile_studio_!TS!.log"
if not exist "!OUT_DIR!\isls-studio\backend" (
    echo          !RED!No backend dir generated!RST!
    set "STUDIO_COMPILE=FAIL"
    set "HAS_ERRORS=1"
    >> "!ERR_LOG!" echo === FEHLER: isls-studio -- kein backend Verzeichnis ===
    >> "!ERR_LOG!" echo Zeitpunkt: !TS_DISPLAY!
    >> "!ERR_LOG!" echo --- Ende ---
    >> "!ERR_LOG!" echo.
    goto :SKIP_STUDIO_COMPILE
)
cd /d "!OUT_DIR!\isls-studio\backend"
cargo build > "!_STUDIO_COMPILE_LOG!" 2>&1
if errorlevel 1 (
    echo          !RED!Compile FAILED!RST!
    set "STUDIO_COMPILE=FAIL"
    set "HAS_ERRORS=1"
    call :LOG_ERROR "isls-studio" "cargo build" "!OUT_DIR!\isls-studio\backend" "!_STUDIO_COMPILE_LOG!"
) else (
    echo          !GRN!PASS!RST!
    set "STUDIO_COMPILE=PASS"
)
cd /d "!ROOT!"

:SKIP_STUDIO_COMPILE

:: --- Check norm seed in migration ---
echo   !CYN![3/3]!RST! Checking norm seed in migration ...
set "_MIGRATION=!OUT_DIR!\isls-studio\backend\migrations\001_initial.sql"
if exist "!_MIGRATION!" (
    findstr /c:"INSERT INTO norms" "!_MIGRATION!" >nul 2>nul
    if not errorlevel 1 (
        echo          !GRN!Norm seed found!RST!
        set "STUDIO_SEED=PASS"
    ) else (
        echo          !YLW!No norm seed in migration!RST!
        set "STUDIO_SEED=FAIL"
    )
) else (
    echo          !YLW!Migration file not found!RST!
    set "STUDIO_SEED=SKIP"
)
echo.

:: ═══════════════════════════════════════════════════════════════════════════
:: Section 7: Norm-Check
:: ═══════════════════════════════════════════════════════════════════════════
echo !BLD![Step 7/8] Norm System Check!RST!
echo.

cd /d "!ROOT!"
set "NORMS_JSON=!USERPROFILE!\.isls\norms.json"

if exist "!NORMS_JSON!" (
    for %%F in ("!NORMS_JSON!") do set "NORMS_SIZE=%%~zF bytes"
    echo   !GRN!norms.json existiert!RST! - !NORMS_SIZE!
) else (
    echo   !YLW!norms.json nicht vorhanden!RST!
)
echo.

echo   --- norms stats ---
"!ISLS_BIN!" norms stats 2>nul
echo.

echo   --- norms candidates ---
"!ISLS_BIN!" norms candidates 2>nul
echo.

echo   --- auto-discovered norms ---
set "AUTO_NORMS=0"
"!ISLS_BIN!" norms list --auto-only > "!LOG_DIR!\norms_auto_!TS!.log" 2>&1
for /f "usebackq delims=" %%L in ("!LOG_DIR!\norms_auto_!TS!.log") do (
    echo %%L | findstr /c:"ISLS-NORM-AUTO-" >nul 2>nul
    if not errorlevel 1 set /a "AUTO_NORMS+=1"
)
if !AUTO_NORMS! GTR 0 (
    echo   !GRN!!AUTO_NORMS! Auto-Norm^(s^) entdeckt!!RST!
    type "!LOG_DIR!\norms_auto_!TS!.log"
) else (
    echo   !YLW!Keine Auto-Norms - erwartet bei wenigen Runs.!RST!
)

:: Count infrastructure norms (D6)
echo.
echo   --- infrastructure norms (D6) ---
set "INFRA_NORMS=0"
"!ISLS_BIN!" norms list > "!LOG_DIR!\norms_all_!TS!.log" 2>&1
for /f "usebackq delims=" %%L in ("!LOG_DIR!\norms_all_!TS!.log") do (
    echo %%L | findstr /c:"ISLS-NORM-INFRA-" >nul 2>nul
    if not errorlevel 1 set /a "INFRA_NORMS+=1"
)
if !INFRA_NORMS! GTR 0 (
    echo   !GRN!!INFRA_NORMS! Infrastructure Norm^(s^) detected!RST!
) else (
    echo   !YLW!No Infrastructure Norms found.!RST!
)

:: Count candidates
set "CANDIDATES=0"
"!ISLS_BIN!" norms candidates > "!LOG_DIR!\norms_cand_!TS!.log" 2>&1
for /f "usebackq delims=" %%L in ("!LOG_DIR!\norms_cand_!TS!.log") do (
    echo %%L | findstr /c:"ISLS-CAND-" >nul 2>nul
    if not errorlevel 1 set /a "CANDIDATES+=1"
)
echo.

:: ═══════════════════════════════════════════════════════════════════════════
:: Section 6: Report
:: ═══════════════════════════════════════════════════════════════════════════
echo !BLD![Step 8/8] Report!RST!
echo.

call :REPORT_LINE ""
call :REPORT_LINE "======================================================"
call :REPORT_LINE " ISLS D6 Pipeline Report -- !TS_DISPLAY!"
call :REPORT_LINE "======================================================"
call :REPORT_LINE ""
call :REPORT_LINE " Workspace Build:    !BUILD_OK!"

if defined TEST_WARNING (
    call :REPORT_LINE " Workspace Tests:    !TEST_PASS! passed, !TEST_FAIL! FAILED - warning"
) else (
    call :REPORT_LINE " Workspace Tests:    !TEST_PASS! passed, !TEST_FAIL! failed"
)

call :REPORT_LINE ""
call :REPORT_LINE " Generated Apps:"
call :REPORT_DOMAIN 0 "petshop"
call :REPORT_DOMAIN 1 "hotel"
call :REPORT_DOMAIN 2 "clinic"
call :REPORT_DOMAIN 3 "gym"
call :REPORT_DOMAIN 4 "school"
call :REPORT_DOMAIN 5 "spa"

call :REPORT_LINE ""
call :REPORT_LINE " Scraping (D5):"
if "!SCRAPE_PETSHOP!"=="PASS" (
    call :REPORT_LINE "   [PASS] petshop scrape"
) else if "!SCRAPE_PETSHOP!"=="FAIL" (
    call :REPORT_LINE "   [FAIL] petshop scrape    -- see logs"
) else (
    call :REPORT_LINE "   [SKIP] petshop scrape"
)
if "!SCRAPE_SELF!"=="PASS" (
    call :REPORT_LINE "   [PASS] isls-self scrape"
) else if "!SCRAPE_SELF!"=="FAIL" (
    call :REPORT_LINE "   [FAIL] isls-self scrape  -- see logs"
) else (
    call :REPORT_LINE "   [SKIP] isls-self scrape"
)
call :REPORT_LINE ""
call :REPORT_LINE " Moebius (D6 forge-self):"
if "!STUDIO_GEN!"=="PASS" (
    call :REPORT_LINE "   [PASS] isls-studio generated"
) else if "!STUDIO_GEN!"=="FAIL" (
    call :REPORT_LINE "   [FAIL] isls-studio generation -- see logs"
) else (
    call :REPORT_LINE "   [SKIP] isls-studio generation"
)
if "!STUDIO_COMPILE!"=="PASS" (
    call :REPORT_LINE "   [PASS] isls-studio compiled"
) else if "!STUDIO_COMPILE!"=="FAIL" (
    call :REPORT_LINE "   [FAIL] isls-studio compile   -- see logs"
) else (
    call :REPORT_LINE "   [SKIP] isls-studio compile"
)
if "!STUDIO_SEED!"=="PASS" (
    call :REPORT_LINE "   [PASS] norm seed in migration"
) else if "!STUDIO_SEED!"=="FAIL" (
    call :REPORT_LINE "   [FAIL] norm seed missing"
) else (
    call :REPORT_LINE "   [SKIP] norm seed check"
)
call :REPORT_LINE ""
call :REPORT_LINE " Norm System:"
call :REPORT_LINE "   Infra-Norms:      !INFRA_NORMS!"
call :REPORT_LINE "   Candidates:       !CANDIDATES!"
call :REPORT_LINE "   Auto-Norms:       !AUTO_NORMS!"
call :REPORT_LINE "   norms.json:       !NORMS_SIZE!"
call :REPORT_LINE ""

set /a "TOTAL=PASS_COUNT+FAIL_COUNT"
if !FAIL_COUNT! EQU 0 (
    call :REPORT_LINE " Result: !PASS_COUNT!/!TOTAL! PASS"
) else (
    call :REPORT_LINE " Result: !PASS_COUNT!/!TOTAL! PASS -- !FAIL_COUNT! failure(s) logged"
)

call :REPORT_LINE ""
call :REPORT_LINE "======================================================"
call :REPORT_LINE ""

:: Hinweis auf Fehler-Logs
if defined HAS_ERRORS (
    if exist "!ERR_LOG!" (
        echo   !YLW!Fehler-Logs kopieren und Claude geben: !ERR_LOG!!RST!
        echo.
    )
)

echo   Report gespeichert: !REPORT_LOG!
echo.

:: ═══════════════════════════════════════════════════════════════════════════
:: Ende
:: ═══════════════════════════════════════════════════════════════════════════
echo !GRN!Pipeline abgeschlossen.!RST!
echo.
pause
endlocal
exit /b 0

:: ═══════════════════════════════════════════════════════════════════════════
:: ABORT -- fataler Fehler, stoppe sofort
:: ═══════════════════════════════════════════════════════════════════════════
:ABORT
echo.
echo !RED!Pipeline abgebrochen. Siehe Logs oben.!RST!
echo.
pause
endlocal
exit /b 1

:: ═══════════════════════════════════════════════════════════════════════════
:: RUN_DOMAIN -- Generiere und kompiliere eine einzelne Domain
::   %1 = Index (0-5), %2 = domain name, %3 = prompt message
:: ═══════════════════════════════════════════════════════════════════════════
:RUN_DOMAIN
set "_IDX=%~1"
set "_DOM=%~2"
set "_PROMPT=%~3"
set /a "_STEP=_IDX+1"

echo   !CYN![!_STEP!/6]!RST! Generating !_DOM! ...

:: --- Generation ---
set "_GEN_LOG=!LOG_DIR!\gen_!_DOM!_!TS!.log"
cd /d "!ROOT!"
"!ISLS_BIN!" forge-chat -m "!_PROMPT!" --api-key !OPENAI_API_KEY! --output "!OUT_DIR!\!_DOM!" > "!_GEN_LOG!" 2>&1
if errorlevel 1 (
    echo          !RED!Generation FAILED!RST!
    set "RESULT_!_IDX!=FAIL"
    set /a "FAIL_COUNT+=1"
    set "HAS_ERRORS=1"
    call :LOG_ERROR "!_DOM!" "isls forge-chat" "!ROOT!" "!_GEN_LOG!"
    goto :eof
)

:: --- Count entities from generated spec.toml ---
set "_ECNT=?"
if exist "!OUT_DIR!\!_DOM!\spec.toml" (
    set "_ECNT=0"
    for /f %%c in ('findstr /c:"[[entities]]" "!OUT_DIR!\!_DOM!\spec.toml" ^| find /c /v ""') do set "_ECNT=%%c"
)
set "ENTITIES_!_IDX!=!_ECNT!"

:: --- Compile generated backend ---
set "_COMPILE_LOG=!LOG_DIR!\compile_!_DOM!_!TS!.log"
if not exist "!OUT_DIR!\!_DOM!\backend" (
    echo          !RED!No backend dir generated!RST!
    set "RESULT_!_IDX!=FAIL"
    set /a "FAIL_COUNT+=1"
    set "HAS_ERRORS=1"
    >> "!ERR_LOG!" echo === FEHLER: !_DOM! -- kein backend Verzeichnis ===
    >> "!ERR_LOG!" echo Zeitpunkt: !TS_DISPLAY!
    >> "!ERR_LOG!" echo Erwartet: !OUT_DIR!\!_DOM!\backend
    >> "!ERR_LOG!" echo --- Ende ---
    >> "!ERR_LOG!" echo.
    goto :eof
)

cd /d "!OUT_DIR!\!_DOM!\backend"
cargo build > "!_COMPILE_LOG!" 2>&1
if errorlevel 1 (
    echo          !RED!Compile FAILED!RST!
    set "RESULT_!_IDX!=FAIL"
    set /a "FAIL_COUNT+=1"
    set "HAS_ERRORS=1"
    call :LOG_ERROR "!_DOM!" "cargo build" "!OUT_DIR!\!_DOM!\backend" "!_COMPILE_LOG!"
    cd /d "!ROOT!"
    goto :eof
)

echo          !GRN!PASS!RST! - !_ECNT! entities
set "RESULT_!_IDX!=PASS"
set /a "PASS_COUNT+=1"
cd /d "!ROOT!"
goto :eof

:: ═══════════════════════════════════════════════════════════════════════════
:: COUNT_TESTS -- Parse cargo test output und setze TEST_PASS / TEST_FAIL
::   %1 = path to test log
:: ═══════════════════════════════════════════════════════════════════════════
:COUNT_TESTS
set "TEST_PASS=0"
set "TEST_FAIL=0"
:: Format: "test result: ok. 47 passed; 0 failed; 0 ignored; ..."
:: Wir extrahieren "X passed" und "Y failed" mit findstr
for /f "tokens=1" %%a in ('findstr /r "[0-9][0-9]* passed" "%~1" 2^>nul ^| findstr /o /r "[0-9][0-9]* passed"') do (
    rem Fallback: simple line count
)
:: Robusterer Ansatz: zaehle alle "X passed" Zahlen
for /f "usebackq delims=" %%L in ("%~1") do (
    set "_LINE=%%L"
    if "!_LINE:passed=!" NEQ "!_LINE!" if "!_LINE:test result=!" NEQ "!_LINE!" (
        for /f "tokens=4,6 delims= " %%a in ("!_LINE!") do (
            set /a "TEST_PASS+=%%a" 2>nul
            set /a "TEST_FAIL+=%%b" 2>nul
        )
    )
)
goto :eof

:: ═══════════════════════════════════════════════════════════════════════════
:: LOG_ERROR -- Schreibe Fehlerblock in errors_TIMESTAMP.log
::   %1 = domain/component, %2 = step, %3 = directory, %4 = log file
:: ═══════════════════════════════════════════════════════════════════════════
:LOG_ERROR
>> "!ERR_LOG!" echo === FEHLER: %~1 -- %~2 ===
>> "!ERR_LOG!" echo Zeitpunkt: !TS_DISPLAY!
>> "!ERR_LOG!" echo Verzeichnis: %~3
>> "!ERR_LOG!" echo --- Output ---
if exist "%~4" type "%~4" >> "!ERR_LOG!"
>> "!ERR_LOG!" echo --- Ende ---
>> "!ERR_LOG!" echo.
goto :eof

:: ═══════════════════════════════════════════════════════════════════════════
:: REPORT_LINE -- Zeile auf Bildschirm UND in Report-Log
::   %1 = text
:: ═══════════════════════════════════════════════════════════════════════════
:REPORT_LINE
echo %~1
>> "!REPORT_LOG!" echo %~1
goto :eof

:: ═══════════════════════════════════════════════════════════════════════════
:: REPORT_DOMAIN -- Zeige PASS/FAIL fuer eine Domain im Report
::   %1 = index, %2 = domain name
:: ═══════════════════════════════════════════════════════════════════════════
:REPORT_DOMAIN
set "_RI=%~1"
set "_RD=%~2"
set "_RR=!RESULT_%_RI%!"
set "_RE=!ENTITIES_%_RI%!"
if "!_RR!"=="PASS" (
    call :REPORT_LINE "   [PASS] !_RD!    -- !_RE! entities, compiled OK"
) else (
    call :REPORT_LINE "   [FAIL] !_RD!    -- see logs for details"
)
goto :eof
