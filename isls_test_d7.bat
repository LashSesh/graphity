@echo off
:: ═══════════════════════════════════════════════════════════════════════════
:: isls_test_d7.bat — D7 REST API Acceptance Test
::
:: Testet alle D7-Endpoints per curl gegen einen laufenden Server.
:: Voraussetzung: isls_studio.bat laeuft in einem anderen Terminal.
:: ═══════════════════════════════════════════════════════════════════════════
chcp 65001 >nul 2>nul
setlocal EnableDelayedExpansion

:: Sicherstellen dass wir im Repo-Root sind
cd /d "%~dp0"

:: ─── ANSI-Farben ─────────────────────────────────────────────────────────
for /F "tokens=1,2 delims=#" %%a in ('"prompt #$E# & echo on & for %%b in (1) do rem"') do set "ESC=%%b"
set "GRN=!ESC![32m"
set "RED=!ESC![31m"
set "YLW=!ESC![33m"
set "CYN=!ESC![36m"
set "BLD=!ESC![1m"
set "DIM=!ESC![90m"
set "RST=!ESC![0m"

set "BASE=http://localhost:8420"
set "PASS=0"
set "FAIL=0"

:: ─── Banner ──────────────────────────────────────────────────────────────
echo.
echo   !BLD!!CYN!======================================================!RST!
echo   !BLD!!CYN!  ISLS D7 REST API Acceptance Test!RST!
echo   !BLD!!CYN!  Architect + Readiness + Forge!RST!
echo   !BLD!!CYN!======================================================!RST!
echo.

:: ─── Pruefe ob curl verfuegbar ist ──────────────────────────────────────
where curl >nul 2>nul
if errorlevel 1 (
    echo   !RED!FEHLER: curl nicht gefunden!!RST!
    echo   curl ist ab Windows 10 dabei. Pruefe deinen PATH.
    goto :ABORT
)

:: ─── Pruefen ob Server laeuft ───────────────────────────────────────────
echo   Pruefe Server...
curl -s -o nul -w "%%{http_code}" !BASE!/health > "%TEMP%\isls_health.txt" 2>nul
set /p HEALTH=<"%TEMP%\isls_health.txt"
if "!HEALTH!" NEQ "200" (
    echo.
    echo   !RED!Server nicht erreichbar auf !BASE!!RST!
    echo   !YLW!Starte zuerst isls_studio.bat in einem anderen Terminal.!RST!
    goto :ABORT
)
echo   !GRN!Server laeuft.!RST!
echo.

:: ─── API-Key ─────────────────────────────────────────────────────────────
set "API_KEY="
if defined OPENAI_API_KEY (
    set "API_KEY=!OPENAI_API_KEY!"
    echo   Verwende OPENAI_API_KEY aus Umgebung.
) else (
    echo   Kein API-Key — Architect arbeitet im Manual-Modus.
)
echo.

:: ═══════════════════════════════════════════════════════════════════════════
:: Test 1: Health Check
:: ═══════════════════════════════════════════════════════════════════════════
echo !BLD![Test 1/9] Health Check!RST!
curl -s !BASE!/api/health > "%TEMP%\isls_t1.json"
type "%TEMP%\isls_t1.json"
echo.
findstr /C:"\"status\":\"ok\"" "%TEMP%\isls_t1.json" >nul 2>nul
if errorlevel 1 (
    echo   !RED!FAIL!RST!
    set /a FAIL+=1
) else (
    echo   !GRN!PASS!RST!
    set /a PASS+=1
)
echo.

:: ═══════════════════════════════════════════════════════════════════════════
:: Test 2: Create Session
:: ═══════════════════════════════════════════════════════════════════════════
echo !BLD![Test 2/9] Create Session!RST!
if "!API_KEY!"=="" (
    curl -s -X POST !BASE!/api/session -H "Content-Type: application/json" -d "{}" > "%TEMP%\isls_t2.json"
) else (
    curl -s -X POST !BASE!/api/session -H "Content-Type: application/json" -d "{\"api_key\":\"!API_KEY!\",\"model\":\"gpt-4o\"}" > "%TEMP%\isls_t2.json"
)
type "%TEMP%\isls_t2.json"
echo.

:: Extrahiere session_id (einfaches parsing)
for /f "tokens=2 delims=:," %%a in ('findstr /C:"session_id" "%TEMP%\isls_t2.json"') do (
    set "RAW_SID=%%a"
)
:: Entferne Anfuehrungszeichen und Leerzeichen
set "SESSION_ID=!RAW_SID:"=!"
set "SESSION_ID=!SESSION_ID: =!"

if "!SESSION_ID!"=="" (
    echo   !RED!FAIL — keine Session-ID erhalten!RST!
    set /a FAIL+=1
    goto :TEST_END
) else (
    echo   !GRN!PASS!RST! — Session: !CYN!!SESSION_ID!!RST!
    set /a PASS+=1
)
echo.

:: ═══════════════════════════════════════════════════════════════════════════
:: Test 3: Readiness (leer — sollte 0% sein)
:: ═══════════════════════════════════════════════════════════════════════════
echo !BLD![Test 3/9] Readiness (leere Session)!RST!
curl -s !BASE!/api/session/!SESSION_ID!/readiness > "%TEMP%\isls_t3.json"
type "%TEMP%\isls_t3.json"
echo.
findstr /C:"\"ready\":false" "%TEMP%\isls_t3.json" >nul 2>nul
if errorlevel 1 (
    echo   !RED!FAIL — erwarte ready=false!RST!
    set /a FAIL+=1
) else (
    echo   !GRN!PASS!RST! — Leere Session ist nicht ready
    set /a PASS+=1
)
echo.

:: ═══════════════════════════════════════════════════════════════════════════
:: Test 4: Send Message 1 — Beschreibung
:: ═══════════════════════════════════════════════════════════════════════════
echo !BLD![Test 4/9] Message 1: App-Beschreibung!RST!
echo   Sende: "Build me a crypto trading journal..."
curl -s -X POST !BASE!/api/session/!SESSION_ID!/message -H "Content-Type: application/json" -d "{\"message\":\"Build me a crypto trading journal with trades, portfolios, and performance metrics\"}" > "%TEMP%\isls_t4.json"
type "%TEMP%\isls_t4.json"
echo.
findstr /C:"\"ok\":true" "%TEMP%\isls_t4.json" >nul 2>nul
if errorlevel 1 (
    echo   !RED!FAIL!RST!
    set /a FAIL+=1
) else (
    echo   !GRN!PASS!RST!
    set /a PASS+=1
)
echo.

:: ═══════════════════════════════════════════════════════════════════════════
:: Test 5: Send Message 2 — Verfeinerung
:: ═══════════════════════════════════════════════════════════════════════════
echo !BLD![Test 5/9] Message 2: Feld-Verfeinerung!RST!
echo   Sende: "Each trade has pair, side, entry_price..."
curl -s -X POST !BASE!/api/session/!SESSION_ID!/message -H "Content-Type: application/json" -d "{\"message\":\"Each trade has pair, side, entry_price, exit_price, quantity, timestamp, and notes\"}" > "%TEMP%\isls_t5.json"
type "%TEMP%\isls_t5.json"
echo.
findstr /C:"\"ok\":true" "%TEMP%\isls_t5.json" >nul 2>nul
if errorlevel 1 (
    echo   !RED!FAIL!RST!
    set /a FAIL+=1
) else (
    echo   !GRN!PASS!RST!
    set /a PASS+=1
)
echo.

:: ═══════════════════════════════════════════════════════════════════════════
:: Test 6: Readiness (nach Messages)
:: ═══════════════════════════════════════════════════════════════════════════
echo !BLD![Test 6/9] Readiness (nach Nachrichten)!RST!
curl -s !BASE!/api/session/!SESSION_ID!/readiness > "%TEMP%\isls_t6.json"
type "%TEMP%\isls_t6.json"
echo.
findstr /C:"\"ok\":true" "%TEMP%\isls_t6.json" >nul 2>nul
if errorlevel 1 (
    echo   !RED!FAIL!RST!
    set /a FAIL+=1
) else (
    echo   !GRN!PASS!RST!
    set /a PASS+=1
)
echo.

:: ═══════════════════════════════════════════════════════════════════════════
:: Test 7: List Sessions
:: ═══════════════════════════════════════════════════════════════════════════
echo !BLD![Test 7/9] List Sessions!RST!
curl -s !BASE!/api/sessions > "%TEMP%\isls_t7.json"
type "%TEMP%\isls_t7.json"
echo.
findstr /C:"\"ok\":true" "%TEMP%\isls_t7.json" >nul 2>nul
if errorlevel 1 (
    echo   !RED!FAIL!RST!
    set /a FAIL+=1
) else (
    echo   !GRN!PASS!RST!
    set /a PASS+=1
)
echo.

:: ═══════════════════════════════════════════════════════════════════════════
:: Test 8: Get Session State
:: ═══════════════════════════════════════════════════════════════════════════
echo !BLD![Test 8/9] Get Session State!RST!
curl -s !BASE!/api/session/!SESSION_ID! > "%TEMP%\isls_t8.json"
findstr /C:"\"ok\":true" "%TEMP%\isls_t8.json" >nul 2>nul
if errorlevel 1 (
    echo   !RED!FAIL!RST!
    type "%TEMP%\isls_t8.json"
    echo.
    set /a FAIL+=1
) else (
    echo   !GRN!PASS!RST! — Session-State abgerufen
    set /a PASS+=1
)
echo.

:: ═══════════════════════════════════════════════════════════════════════════
:: Test 9: Norms API
:: ═══════════════════════════════════════════════════════════════════════════
echo !BLD![Test 9/9] Norms API (Baseline)!RST!
curl -s !BASE!/api/norms > "%TEMP%\isls_t9.json"
findstr /C:"\"ok\":true" "%TEMP%\isls_t9.json" >nul 2>nul
if errorlevel 1 (
    echo   !RED!FAIL!RST!
    set /a FAIL+=1
) else (
    echo   !GRN!PASS!RST! — Norm-API erreichbar
    set /a PASS+=1
)
echo.

:: ═══════════════════════════════════════════════════════════════════════════
:TEST_END
:: ═══════════════════════════════════════════════════════════════════════════
echo.
echo   !BLD!======================================================!RST!
echo   !BLD!  D7 REST API Test — Ergebnis!RST!
echo   !BLD!======================================================!RST!
echo.
echo   !GRN!PASS: !PASS!!RST!
echo   !RED!FAIL: !FAIL!!RST!
echo.

set /a TOTAL=!PASS!+!FAIL!
if !FAIL! EQU 0 (
    echo   !BLD!!GRN!Alle !TOTAL! Tests bestanden!!RST!
) else (
    echo   !BLD!!YLW!!FAIL! von !TOTAL! Tests fehlgeschlagen.!RST!
)
echo.

:: ─── Optionaler Forge-Test ───────────────────────────────────────────────
echo   ────────────────────────────────────────────────────
echo.
set "_FORGE="
set /p "_FORGE=  Forge-Test ausfuehren? (j/N): "
if /i "!_FORGE!" NEQ "j" goto :DONE

echo.
echo   !BLD!Forge-Test!RST!
echo   Starte Forge fuer Session !SESSION_ID!...
curl -s -X POST !BASE!/api/session/!SESSION_ID!/forge -H "Content-Type: application/json" -d "{}" > "%TEMP%\isls_forge.json"
type "%TEMP%\isls_forge.json"
echo.
echo.

findstr /C:"\"ok\":true" "%TEMP%\isls_forge.json" >nul 2>nul
if errorlevel 1 (
    echo   !RED!Forge konnte nicht gestartet werden.!RST!
    findstr /C:"error" "%TEMP%\isls_forge.json"
    echo.
) else (
    echo   !GRN!Forge gestartet!!RST!
    echo   Der Forge laeuft im Hintergrund.
    echo.
    echo   Warte 15 Sekunden auf Ergebnis...
    timeout /t 15 /nobreak >nul
    curl -s !BASE!/api/session/!SESSION_ID! > "%TEMP%\isls_forge_check.json"
    findstr /C:"forge_result" "%TEMP%\isls_forge_check.json" >nul 2>nul
    if errorlevel 1 (
        echo   !YLW!Forge laeuft noch oder ist fehlgeschlagen.!RST!
        echo   Pruefe manuell mit: curl !BASE!/api/session/!SESSION_ID!
    ) else (
        echo   !GRN!Forge abgeschlossen!!RST!
        findstr /C:"files_generated" "%TEMP%\isls_forge_check.json"
        findstr /C:"total_loc" "%TEMP%\isls_forge_check.json"
        findstr /C:"output_dir" "%TEMP%\isls_forge_check.json"
    )
)

:DONE
echo.
echo   ────────────────────────────────────────────────────
echo   Session-ID: !SESSION_ID!
echo   Server:     !BASE!
echo   Studio:     !BASE!/studio
echo.
pause
endlocal
exit /b 0

:: ═══════════════════════════════════════════════════════════════════════════
:: ABORT — Fehler, warte auf Tastendruck damit das Fenster offen bleibt
:: ═══════════════════════════════════════════════════════════════════════════
:ABORT
echo.
echo !RED!Abgebrochen. Siehe Fehler oben.!RST!
echo.
pause
endlocal
exit /b 1
