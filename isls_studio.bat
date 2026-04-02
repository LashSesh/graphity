@echo off
:: ═══════════════════════════════════════════════════════════════════════════
:: isls_studio.bat — One-Click ISLS Studio Launcher
::
:: Doppelklick genuegt. Fragt API-Key ab, baut falls noetig,
:: oeffnet den Browser und startet den Studio-Server.
:: CTRL+C beendet den Server sauber.
:: ═══════════════════════════════════════════════════════════════════════════
chcp 65001 >nul 2>nul
setlocal EnableDelayedExpansion

:: Sicherstellen dass wir im Repo-Root sind (wichtig bei Doppelklick)
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

:: ─── Pfade ───────────────────────────────────────────────────────────────
set "ROOT=%~dp0"
if "!ROOT:~-1!"=="\" set "ROOT=!ROOT:~0,-1!"
set "ISLS_BIN=!ROOT!\target\release\isls.exe"
set "PORT=8420"

:: ─── Banner ──────────────────────────────────────────────────────────────
echo.
echo   !BLD!!CYN!======================================================!RST!
echo   !BLD!!CYN!  ISLS Studio Launcher — D7 Cockpit!RST!
echo   !BLD!!CYN!  Architekt-Modus + Drucker-Modus!RST!
echo   !BLD!!CYN!======================================================!RST!
echo.

:: ═══════════════════════════════════════════════════════════════════════════
:: Step 1: API-Key
:: ═══════════════════════════════════════════════════════════════════════════
echo !BLD![1/3] API-Key!RST!
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
        echo   !YLW!Kein API-Key — Studio startet im Mock-Modus.!RST!
        echo   Architect-Modus benoetigt einen API-Key fuer LLM-Gespraeche.
        echo   Forge laeuft trotzdem (Mock-Oracle^).
    ) else (
        echo   !GRN!Key gesetzt.!RST!
    )
)
echo.

:: ═══════════════════════════════════════════════════════════════════════════
:: Step 2: Build (falls noetig)
:: ═══════════════════════════════════════════════════════════════════════════
echo !BLD![2/3] Build!RST!
echo.

if exist "!ISLS_BIN!" (
    echo   isls.exe vorhanden — ueberspringe Build.
    echo   Fuer Neubau: del target\release\isls.exe
) else (
    :: Pruefe ob cargo verfuegbar ist
    where cargo >nul 2>nul
    if errorlevel 1 (
        echo.
        echo   !RED!FEHLER: cargo nicht gefunden!!RST!
        echo.
        echo   Rust muss installiert sein. Installiere von:
        echo   https://rustup.rs/
        echo.
        goto :ABORT
    )

    echo   cargo build --workspace --release ...
    echo   Das kann beim ersten Mal 2-3 Minuten dauern.
    echo.
    cargo build --workspace --release
    if errorlevel 1 (
        echo.
        echo   !RED!BUILD FEHLGESCHLAGEN!RST!
        echo   !YLW!Pruefe die Fehlermeldungen oben.!RST!
        echo.
        goto :ABORT
    )
    echo   !GRN!BUILD OK!RST!
)
echo.

:: Nochmal pruefen ob die exe jetzt wirklich da ist
if not exist "!ISLS_BIN!" (
    echo   !RED!FEHLER: !ISLS_BIN! nicht gefunden!!RST!
    echo.
    echo   Build war erfolgreich aber die Binary fehlt.
    echo   Versuche: cargo build --workspace --release
    echo.
    goto :ABORT
)

:: ═══════════════════════════════════════════════════════════════════════════
:: Step 3: Browser oeffnen + Server starten
:: ═══════════════════════════════════════════════════════════════════════════
echo !BLD![3/3] Studio starten!RST!
echo.
echo   !CYN!Studio:    http://localhost:!PORT!/studio!RST!
echo   !CYN!Architect: Klicke auf AR in der Sidebar!RST!
echo   !CYN!API:       http://localhost:!PORT!/!RST!
echo   !CYN!WebSocket: ws://localhost:!PORT!/events!RST!
echo.
echo   !YLW!CTRL+C druecken zum Beenden.!RST!
echo.
echo   ────────────────────────────────────────────────────
echo.

:: Browser oeffnen (BEVOR der Server startet, da serve blockiert)
start "" "http://localhost:!PORT!/studio"

:: Server im Vordergrund starten (blockiert bis CTRL+C)
if "!OPENAI_API_KEY!"=="" (
    "!ISLS_BIN!" serve --port !PORT!
) else (
    "!ISLS_BIN!" serve --port !PORT! --api-key "!OPENAI_API_KEY!"
)

:: Wenn wir hier ankommen, ist der Server beendet (CTRL+C oder Fehler)
echo.
echo   Server beendet.
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
