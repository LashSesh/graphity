@echo off
:: ===========================================================================
:: isls_fulltest.bat -- ISLS Full System Test + Metrics Accumulation
::
:: Tests ALL systems: D1-D8 pipeline, M1 Swarm, S1 Studio, I1 Infogenetik.
:: Generates 18 domains + Moebius, scrapes 4 projects, checks norms/fitness.
:: Fills ~/.isls/metrics.jsonl and ~/.isls/fitness.json with real data.
:: ===========================================================================
chcp 65001 >nul 2>nul
setlocal EnableDelayedExpansion

:: --- Pfade ---
set "ROOT=%~dp0"
if "%ROOT:~-1%"=="\" set "ROOT=%ROOT:~0,-1%"

:: --- Timestamp (locale-independent via wmic) ---
for /f "skip=1 tokens=1" %%t in ('wmic os get localdatetime 2^>nul') do (
    set "DT=%%t"
    goto :GOT_DT
)
:GOT_DT
set "TS=%DT:~0,4%%DT:~4,2%%DT:~6,2%_%DT:~8,2%%DT:~10,2%"
set "TS_DISPLAY=%DT:~0,4%-%DT:~4,2%-%DT:~6,2% %DT:~8,2%:%DT:~10,2%"

set "LOG_DIR=%ROOT%\logs"
set "OUT_DIR=%ROOT%\output\fulltest_%TS%"
set "ERR_LOG=%LOG_DIR%\fulltest_errors_%TS%.log"
set "REPORT_LOG=%LOG_DIR%\fulltest_report_%TS%.log"
set "ISLS_BIN=%ROOT%\target\release\isls.exe"

:: --- Ensure ALL directories exist ---
if not exist "%LOG_DIR%" mkdir "%LOG_DIR%"
if not exist "%OUT_DIR%" mkdir "%OUT_DIR%"

:: --- Zaehler ---
set "PASS_COUNT=0"
set "FAIL_COUNT=0"
set "TEST_PASS=0"
set "TEST_FAIL=0"
set "BUILD_OK="
set "HAS_ERRORS="
set "CANDIDATES=0"
set "AUTO_NORMS=0"
set "INFRA_NORMS=0"

:: Domain results (18 domains)
for /L %%i in (0,1,17) do (
    set "RESULT_%%i=SKIP"
    set "ENTITIES_%%i=?"
    set "DNAME_%%i=?"
)

:: Scrape results
set "SCRAPE_0=SKIP"
set "SCRAPE_1=SKIP"
set "SCRAPE_2=SKIP"
set "SCRAPE_SELF=SKIP"

:: Moebius
set "STUDIO_GEN=SKIP"
set "STUDIO_COMPILE=SKIP"

:: ===========================================================================
:: Header + Cost Warning
:: ===========================================================================
echo.
echo ======================================================
echo   ISLS Full System Test + Metrics Accumulation
echo   %TS_DISPLAY%
echo ======================================================
echo.
echo   WARNUNG: Dieser Test generiert 19 Applikationen via OpenAI API.
echo   Geschaetzte Kosten: ~$2.85 (19 x ~$0.15)
echo.
set /p "_CONFIRM=  Fortfahren? (j/N): "
if /i "%_CONFIRM%" NEQ "j" (
    echo   Abgebrochen.
    goto :ABORT
)
echo.

:: ===========================================================================
:: Phase 1: Setup
:: ===========================================================================
echo [Phase 1/8] Setup
echo.

:: --- API-Key ---
if defined OPENAI_API_KEY (
    echo   Vorhandener Key gefunden.
    set "_USE="
    set /p "_USE=  Enter = verwenden, oder neuen Key eingeben: "
    if "!_USE!" NEQ "" set "OPENAI_API_KEY=!_USE!"
    echo   [OK] Key aktiv.
) else (
    set /p "OPENAI_API_KEY=  OpenAI API Key (sk-...): "
    if "!OPENAI_API_KEY!"=="" (
        echo   [FAIL] Kein API-Key. Abbruch.
        goto :ABORT
    )
    echo   [OK] Key gesetzt.
)
echo.

:: --- Build ---
echo   cargo build --workspace --release ...
set "BUILD_LOG=%LOG_DIR%\fulltest_build_%TS%.log"
cd /d "%ROOT%"
cargo build --workspace --release > "%BUILD_LOG%" 2>&1
if errorlevel 1 (
    set "BUILD_OK=FAIL"
    echo   [FAIL] BUILD FEHLGESCHLAGEN
    echo   Log: %BUILD_LOG%
    call :LOG_ERROR "workspace" "cargo build" "%ROOT%" "%BUILD_LOG%"
    goto :ABORT
)
set "BUILD_OK=PASS"
echo   [OK] BUILD PASS
echo.

:: --- Tests ---
echo   cargo test --workspace ...
set "TEST_LOG=%LOG_DIR%\fulltest_test_%TS%.log"
cd /d "%ROOT%"
cargo test --workspace > "%TEST_LOG%" 2>&1

:: Parse test results: sum all "X passed" and "Y failed" from test result lines
set "TEST_PASS=0"
set "TEST_FAIL=0"
for /f "usebackq delims=" %%L in ("%TEST_LOG%") do (
    set "_LINE=%%L"
    echo !_LINE! | findstr /c:"test result" >nul 2>nul
    if not errorlevel 1 (
        for /f "tokens=4,6 delims= " %%a in ("!_LINE!") do (
            set /a "TEST_PASS+=%%a" 2>nul
            set /a "TEST_FAIL+=%%b" 2>nul
        )
    )
)
if %TEST_FAIL% GTR 0 (
    echo   [WARN] %TEST_PASS% passed, %TEST_FAIL% FAILED
    set "HAS_ERRORS=1"
) else (
    echo   [OK] %TEST_PASS% passed, 0 failed
)
echo.

:: ===========================================================================
:: Phase 2: D4-Pipeline (6 Core Domains)
:: ===========================================================================
echo [Phase 2/8] D4 Pipeline -- 6 Core Domains
echo.

set "DNAME_0=petshop"
set "DNAME_1=hotel"
set "DNAME_2=clinic"
set "DNAME_3=gym"
set "DNAME_4=school"
set "DNAME_5=spa"

call :RUN_DOMAIN 0 "petshop" "Pet shop with animals, owners, and veterinary appointments"
call :RUN_DOMAIN 1 "hotel" "Hotel management with rooms, guests, bookings, and housekeeping tasks"
call :RUN_DOMAIN 2 "clinic" "Medical clinic with patients, doctors, appointments, and prescriptions"
call :RUN_DOMAIN 3 "gym" "Fitness gym with members, trainers, workout classes, and membership plans"
call :RUN_DOMAIN 4 "school" "School management with students, teachers, courses, and grades"
call :RUN_DOMAIN 5 "spa" "Spa and wellness center with treatments, therapists, bookings, and packages"
echo.

:: ===========================================================================
:: Phase 3: Extended Domains (12 more)
:: ===========================================================================
echo [Phase 3/8] Extended Domains -- 12 Additional
echo.

set "DNAME_6=restaurant"
set "DNAME_7=library"
set "DNAME_8=carrental"
set "DNAME_9=events"
set "DNAME_10=realestate"
set "DNAME_11=elearning"
set "DNAME_12=vetclinic"
set "DNAME_13=warehouse"
set "DNAME_14=blog"
set "DNAME_15=fitness"
set "DNAME_16=recipes"
set "DNAME_17=taskmanager"

call :RUN_DOMAIN 6 "restaurant" "Restaurant management with menus, tables, reservations, and staff"
call :RUN_DOMAIN 7 "library" "Library system with books, members, loans, and categories"
call :RUN_DOMAIN 8 "carrental" "Car rental service with vehicles, customers, bookings, and payments"
call :RUN_DOMAIN 9 "events" "Event management with events, venues, tickets, and attendees"
call :RUN_DOMAIN 10 "realestate" "Real estate platform with properties, agents, viewings, and offers"
call :RUN_DOMAIN 11 "elearning" "Online learning platform with courses, students, lessons, and quizzes"
call :RUN_DOMAIN 12 "vetclinic" "Veterinary clinic with pets, owners, appointments, and treatments"
call :RUN_DOMAIN 13 "warehouse" "Warehouse inventory with products, locations, stock movements, and orders"
call :RUN_DOMAIN 14 "blog" "Blog platform with posts, authors, comments, and tags"
call :RUN_DOMAIN 15 "fitness" "Fitness tracker with workouts, exercises, progress logs, and goals"
call :RUN_DOMAIN 16 "recipes" "Recipe manager with recipes, ingredients, meal plans, and shopping lists"
call :RUN_DOMAIN 17 "taskmanager" "Task management with projects, tasks, milestones, and team members"
echo.

:: ===========================================================================
:: Phase 4: D6 Moebius (forge-self)
:: ===========================================================================
echo [Phase 4/8] D6 Moebius -- forge-self
echo.

echo   Generating ISLS Studio ...
set "_STUDIO_LOG=%LOG_DIR%\fulltest_studio_%TS%.log"
cd /d "%ROOT%"
"%ISLS_BIN%" forge-self --api-key %OPENAI_API_KEY% --output "%OUT_DIR%\isls-studio" > "%_STUDIO_LOG%" 2>&1
if errorlevel 1 (
    echo   [FAIL] Generation FAILED
    set "STUDIO_GEN=FAIL"
    set "HAS_ERRORS=1"
    call :LOG_ERROR "isls-studio" "forge-self" "%ROOT%" "%_STUDIO_LOG%"
    goto :SKIP_STUDIO
)
set "STUDIO_GEN=PASS"
echo   [OK] Generation PASS

echo   Compiling generated Studio ...
set "_SC_LOG=%LOG_DIR%\fulltest_studio_compile_%TS%.log"
if exist "%OUT_DIR%\isls-studio\backend" (
    cd /d "%OUT_DIR%\isls-studio\backend"
    cargo build > "%_SC_LOG%" 2>&1
    if errorlevel 1 (
        echo   [FAIL] Compile FAILED
        set "STUDIO_COMPILE=FAIL"
        set "HAS_ERRORS=1"
        call :LOG_ERROR "isls-studio" "cargo build" "%OUT_DIR%\isls-studio\backend" "%_SC_LOG%"
    ) else (
        echo   [OK] Compile PASS
        set "STUDIO_COMPILE=PASS"
    )
    cd /d "%ROOT%"
) else (
    echo   [FAIL] No backend directory
    set "STUDIO_COMPILE=FAIL"
)
:SKIP_STUDIO
echo.

:: ===========================================================================
:: Phase 5: D5 Scraping
:: ===========================================================================
echo [Phase 5/8] D5 Scraping
echo.

call :RUN_SCRAPE 0 "petshop"
call :RUN_SCRAPE 1 "restaurant"
call :RUN_SCRAPE 2 "library"

:: Scrape ISLS source
echo   [4/4] Scraping ISLS source ...
set "_SL=%LOG_DIR%\fulltest_scrape_self_%TS%.log"
cd /d "%ROOT%"
"%ISLS_BIN%" scrape --path "%ROOT%" --domain "isls-self" > "%_SL%" 2>&1
if errorlevel 1 (
    echo          [FAIL]
    set "SCRAPE_SELF=FAIL"
    set "HAS_ERRORS=1"
) else (
    echo          [OK]
    set "SCRAPE_SELF=PASS"
)
echo.

:: ===========================================================================
:: Phase 6: Norm-System Check
:: ===========================================================================
echo [Phase 6/8] Norm System Check
echo.

cd /d "%ROOT%"
echo   --- norms list ---
"%ISLS_BIN%" norms list 2>nul
echo.
echo   --- norms stats ---
"%ISLS_BIN%" norms stats 2>nul
echo.
echo   --- norms candidates ---
"%ISLS_BIN%" norms candidates 2>nul
echo.
echo   --- norms fitness (I1) ---
"%ISLS_BIN%" norms fitness 2>nul
echo.

:: Count auto-norms
set "AUTO_NORMS=0"
"%ISLS_BIN%" norms list --auto-only > "%LOG_DIR%\ft_norms_auto_%TS%.log" 2>&1
for /f "usebackq delims=" %%L in ("%LOG_DIR%\ft_norms_auto_%TS%.log") do (
    echo %%L | findstr /c:"ISLS-NORM-AUTO-" >nul 2>nul
    if not errorlevel 1 set /a "AUTO_NORMS+=1"
)

:: Count infra-norms
set "INFRA_NORMS=0"
"%ISLS_BIN%" norms list > "%LOG_DIR%\ft_norms_all_%TS%.log" 2>&1
for /f "usebackq delims=" %%L in ("%LOG_DIR%\ft_norms_all_%TS%.log") do (
    echo %%L | findstr /c:"ISLS-NORM-INFRA-" >nul 2>nul
    if not errorlevel 1 set /a "INFRA_NORMS+=1"
)

:: Count candidates
set "CANDIDATES=0"
"%ISLS_BIN%" norms candidates > "%LOG_DIR%\ft_norms_cand_%TS%.log" 2>&1
for /f "usebackq delims=" %%L in ("%LOG_DIR%\ft_norms_cand_%TS%.log") do (
    echo %%L | findstr /c:"ISLS-CAND-" >nul 2>nul
    if not errorlevel 1 set /a "CANDIDATES+=1"
)

:: ===========================================================================
:: Phase 7: Metriken-Check (I1)
:: ===========================================================================
echo [Phase 7/8] Metriken-Check (I1)
echo.

cd /d "%ROOT%"
echo   --- metrics summary ---
"%ISLS_BIN%" metrics 2>nul
echo.
echo   --- metrics last 5 ---
"%ISLS_BIN%" metrics --last 5 2>nul
echo.

:: Check files exist
set "METRICS_EXIST=NEIN"
set "FITNESS_EXIST=NEIN"
set "METRICS_FILE=%USERPROFILE%\.isls\metrics.jsonl"
set "FITNESS_FILE=%USERPROFILE%\.isls\fitness.json"

if exist "%METRICS_FILE%" (
    set "METRICS_EXIST=JA"
    for %%F in ("%METRICS_FILE%") do echo   metrics.jsonl: vorhanden (%%~zF bytes)
) else (
    echo   metrics.jsonl: nicht vorhanden
)

if exist "%FITNESS_FILE%" (
    set "FITNESS_EXIST=JA"
    for %%F in ("%FITNESS_FILE%") do echo   fitness.json:  vorhanden (%%~zF bytes)
) else (
    echo   fitness.json:  nicht vorhanden
)
echo.

:: ===========================================================================
:: Phase 8: Report
:: ===========================================================================
echo [Phase 8/8] Report
echo.

call :RL ""
call :RL "======================================================"
call :RL " ISLS Full System Test -- %TS_DISPLAY%"
call :RL "======================================================"
call :RL ""
call :RL " Build:              %BUILD_OK% (%TEST_PASS% tests)"
call :RL ""
call :RL " Generation (18 Domains):"

for /L %%i in (0,1,17) do (
    call :REPORT_DOMAIN %%i
)

set /a "TOTAL=PASS_COUNT+FAIL_COUNT"
call :RL "   Result: %PASS_COUNT%/%TOTAL% PASS"
call :RL ""

call :RL " Moebius (forge-self):"
if "%STUDIO_GEN%"=="PASS" (
    call :RL "   [PASS] isls-studio generated"
) else (
    call :RL "   [FAIL] isls-studio generation"
)
if "%STUDIO_COMPILE%"=="PASS" (
    call :RL "   [PASS] isls-studio compiled"
) else if "%STUDIO_COMPILE%"=="FAIL" (
    call :RL "   [FAIL] isls-studio compile"
) else (
    call :RL "   [SKIP] isls-studio compile"
)
call :RL ""

call :RL " Scraping:"
call :RL "   [%SCRAPE_0%] petshop"
call :RL "   [%SCRAPE_1%] restaurant"
call :RL "   [%SCRAPE_2%] library"
call :RL "   [%SCRAPE_SELF%] isls-self"
call :RL ""

call :RL " Norm System:"
call :RL "   Auto-Norms:       %AUTO_NORMS%"
call :RL "   Infra-Norms:      %INFRA_NORMS%"
call :RL "   Candidates:       %CANDIDATES%"
call :RL ""

call :RL " Metriken (I1):"
call :RL "   metrics.jsonl:    %METRICS_EXIST%"
call :RL "   fitness.json:     %FITNESS_EXIST%"
call :RL ""

call :RL " API Budget:"
set /a "_COST_CENTS=TOTAL*15+15"
call :RL "   Geschaetzte Kosten: ~%_COST_CENTS% cents (%TOTAL% domains + 1 Moebius)"
call :RL ""
call :RL "======================================================"
call :RL ""

if defined HAS_ERRORS (
    if exist "%ERR_LOG%" (
        echo   Fehler-Logs: %ERR_LOG%
    )
)
echo   Report: %REPORT_LOG%
echo.
echo Full System Test abgeschlossen.
echo.
pause
endlocal
exit /b 0

:: ===========================================================================
:: ABORT
:: ===========================================================================
:ABORT
echo.
echo Test abgebrochen.
echo.
pause
endlocal
exit /b 1

:: ===========================================================================
:: RUN_DOMAIN -- Generate + compile a domain
::   %1=index %2=name %3=prompt
:: ===========================================================================
:RUN_DOMAIN
set "_IDX=%~1"
set "_DOM=%~2"
set "_PROMPT=%~3"
set /a "_STEP=_IDX+1"

echo   [%_STEP%/18] %_DOM% ...

:: Ensure output dir exists
if not exist "%OUT_DIR%\%_DOM%" mkdir "%OUT_DIR%\%_DOM%"

set "_GEN_LOG=%LOG_DIR%\ft_gen_%_DOM%_%TS%.log"
cd /d "%ROOT%"
"%ISLS_BIN%" forge-chat -m "%_PROMPT%" --api-key %OPENAI_API_KEY% --output "%OUT_DIR%\%_DOM%" > "%_GEN_LOG%" 2>&1
if errorlevel 1 (
    echo          [FAIL] generation
    set "RESULT_%_IDX%=FAIL"
    set /a "FAIL_COUNT+=1"
    set "HAS_ERRORS=1"
    call :LOG_ERROR "%_DOM%" "forge-chat" "%ROOT%" "%_GEN_LOG%"
    goto :eof
)

:: Count entities
set "_ECNT=?"
if exist "%OUT_DIR%\%_DOM%\spec.toml" (
    set "_ECNT=0"
    for /f %%c in ('findstr /c:"[[entities]]" "%OUT_DIR%\%_DOM%\spec.toml" ^| find /c /v ""') do set "_ECNT=%%c"
)
set "ENTITIES_%_IDX%=!_ECNT!"

:: Compile check
set "_CL=%LOG_DIR%\ft_compile_%_DOM%_%TS%.log"
if not exist "%OUT_DIR%\%_DOM%\backend" (
    echo          [FAIL] no backend dir
    set "RESULT_%_IDX%=FAIL"
    set /a "FAIL_COUNT+=1"
    set "HAS_ERRORS=1"
    goto :eof
)
cd /d "%OUT_DIR%\%_DOM%\backend"
cargo build > "%_CL%" 2>&1
if errorlevel 1 (
    echo          [FAIL] compile
    set "RESULT_%_IDX%=FAIL"
    set /a "FAIL_COUNT+=1"
    set "HAS_ERRORS=1"
    call :LOG_ERROR "%_DOM%" "cargo build" "%OUT_DIR%\%_DOM%\backend" "%_CL%"
    cd /d "%ROOT%"
    goto :eof
)
echo          [OK] %_ECNT% entities
set "RESULT_%_IDX%=PASS"
set /a "PASS_COUNT+=1"
cd /d "%ROOT%"
goto :eof

:: ===========================================================================
:: RUN_SCRAPE -- Scrape a generated project
::   %1=index %2=name
:: ===========================================================================
:RUN_SCRAPE
set "_SI=%~1"
set "_SN=%~2"
set /a "_SS=_SI+1"

echo   [%_SS%/4] Scraping %_SN% ...
set "_SL=%LOG_DIR%\ft_scrape_%_SN%_%TS%.log"
cd /d "%ROOT%"
if exist "%OUT_DIR%\%_SN%" (
    "%ISLS_BIN%" scrape --path "%OUT_DIR%\%_SN%" > "%_SL%" 2>&1
    if errorlevel 1 (
        echo          [FAIL]
        set "SCRAPE_%_SI%=FAIL"
        set "HAS_ERRORS=1"
    ) else (
        echo          [OK]
        set "SCRAPE_%_SI%=PASS"
    )
) else (
    echo          [SKIP] not generated
)
goto :eof

:: ===========================================================================
:: LOG_ERROR
:: ===========================================================================
:LOG_ERROR
>> "%ERR_LOG%" echo === FEHLER: %~1 -- %~2 ===
>> "%ERR_LOG%" echo Zeitpunkt: %TS_DISPLAY%
>> "%ERR_LOG%" echo Verzeichnis: %~3
>> "%ERR_LOG%" echo --- Output ---
if exist "%~4" type "%~4" >> "%ERR_LOG%"
>> "%ERR_LOG%" echo --- Ende ---
>> "%ERR_LOG%" echo.
goto :eof

:: ===========================================================================
:: RL -- Report Line (screen + file)
:: ===========================================================================
:RL
echo %~1
>> "%REPORT_LOG%" echo %~1
goto :eof

:: ===========================================================================
:: REPORT_DOMAIN
:: ===========================================================================
:REPORT_DOMAIN
set "_RI=%~1"
set "_RD=!DNAME_%_RI%!"
set "_RR=!RESULT_%_RI%!"
set "_RE=!ENTITIES_%_RI%!"
if "!_RR!"=="PASS" (
    call :RL "   [PASS] !_RD!    -- !_RE! entities, compiled"
) else if "!_RR!"=="FAIL" (
    call :RL "   [FAIL] !_RD!    -- see logs"
) else (
    call :RL "   [SKIP] !_RD!"
)
goto :eof
