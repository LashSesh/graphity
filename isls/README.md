# ISLS — Intelligent Semantic Ledger Substrate

![Version](https://img.shields.io/badge/version-1.0.0-blue)
![License](https://img.shields.io/badge/license-MIT-green)
![Rust](https://img.shields.io/badge/rust-2021-orange)

Deterministisches, append-only Observations-zu-Code-System mit kristall-basierter Wissensakkumulation und Multi-Pass LLM-Enrichment.

---

## Was ist ISLS?

ISLS verarbeitet Rohdaten in einer formalen 5-stufigen Pipeline (L0–L4): Beobachtungen werden aufgenommen, in einem unveränderlichen Hash-DAG persistiert, auf Constraints analysiert, durch dualen Konsens validiert und schließlich als semantische Kristalle emittiert. Alle Artefakte sind SHA-256-content-addressed und vollständig replay-fähig.

Mit Version 2.1 kommt der **Multi-Pass Render Loop** hinzu: Ein vollständiger Softwarestack wird zunächst strukturell aus Kristall-Templates aufgebaut und dann in bis zu 5 LLM-Pässen schrittweise angereichert — von Domain-Logik über Edge Cases bis zu automatisch generierten Tests. Die **Universal Crystal Registry** mit 15 eingebauten Mustern akkumuliert dabei Wissen über Generierungsläufe hinweg.

21 konstitutionelle Constraints (ADAMANT Protocol, C4-Klasse) sind unveränderlich in das System eingebettet.

---

## Architektur

```
ISLS v1.0.0 — 42 Crates
═══════════════════════════════════════════════════════════════

Tier 1 — Core Pipeline (L0–L4)
  isls-observe    L0  Dateningestion, Kanonisierung
  isls-persist    L1  Hash-DAG, append-only, 5D-State
  isls-extract    L2  Constraint-Mining aus Graph
  isls-consensus  L3  Dualer Konsens + Proof-of-Reproducibility
  isls-engine         Orchestrierung der Pipeline (C9)
  isls-carrier        Informationsgeometrischer Carrier
  isls-archive        Kristallarchiv, Hash-verkettete Evidenz
  isls-morph      L4  Morphogene Graphtransformationen

Tier 2 — Infrastructure
  isls-harness        Validierung, Benchmarks, Metriken
  isls-registry       Digest-gebundener Operator-Katalog
  isls-manifest       Content-addressed Lauf-Metadaten
  isls-capsule        Secret Release (OLP)
  isls-scheduler      Adaptiver Tick-Scheduler
  isls-topology       Spektrale Graphanalyse, CTQW, Kuramoto
  isls-store          SQLite-Persistenz (eingebettet)
  isls-scale          Multi-Skalen-Beobachtung (Hypercube)

Tier 3 — Generative Synthesis
  isls-cli            Einzelnes Binary, alle Befehle
  isls-gateway        REST + WebSocket + Studio-Web-UI (Port 8420)
  isls-pmhd           Adversariales Hypothesen-Drill
  isls-artifact-ir    Universelles Artefakt-IR
  isls-forge          Generative Syntheseengine
  isls-compose        Rekursive Dekomposition & Komposition
  isls-oracle         LLM-Interface (OpenAI / Anthropic)
  isls-templates      Kristallisierte Architekturmuster
  isls-foundry        Closed-Loop Software-Fabrikation
  isls-multilang      Babylon Bridge: Multi-Language Forge
  isls-navigator      Spektral-geführter Musterraum-Explorer
  isls-agent          Autonomer zielgerichteter Agent
  isls-swarm          Multi-Agent Swarm-Koordinator

Tier 4 — V2.1 Full-Stack Autonomy
  isls-renderloop     6-Pass inkrementelles LLM-Enrichment   ← NEU
  isls-orchestrator   11-stufige Generierungspipeline
  isls-hypercube      Requirement-Space-Dekomposition
  isls-decomposer     Rekursive Architekturzerlegung
  isls-planner        AppSpec-Parsing & Architekturplanung
  isls-blueprint      Code-Generierungsmuster
  isls-reader         Multi-Language Source Code Parser
  isls-code-topo      Strukturanalyse
  isls-learner        Pattern-Akkumulation
  isls-deployer       Docker / Compose-Generierung
  isls-integrator     Cross-Modul-Schnittstellenprüfung
  isls-langgen        Grammatik- und Sprachgenerierung
```

---

## Quick Start

```bash
# 1. Build
cd isls
cargo build --release

# 2. Initialisieren
./target/release/isls init

# 3. Systemstatus
./target/release/isls status

# 4. Warehouse-System generieren (offline, kein API-Key nötig)
./target/release/isls forge-v2 \
  --requirements examples/warehouse.toml \
  --output ./output/warehouse \
  --mock-oracle --passes 5 --trace

# 5. Mit echtem OpenAI-Key (1 Pass = minimale Kosten)
./target/release/isls forge-v2 \
  --requirements examples/warehouse.toml \
  --output ./output/warehouse-live \
  --api-key $OPENAI_API_KEY --passes 1

# 6. Crystal Registry anzeigen
./target/release/isls crystals list

# 7. Web-UI starten
./target/release/isls serve --port 8420
```

---

## CLI-Referenz

| Gruppe | Befehle |
|--------|---------|
| System | `init`, `status`, `help` |
| Ingestion | `ingest --adapter <name> [--scenario <name>]` |
| Pipeline | `run [--mode shadow\|live]`, `execute --input <path>` |
| Validierung | `validate [--formal] [--retro]`, `report [--json\|--html]`, `bench` |
| Kapsel (OLP) | `seal --secret <text>`, `open --capsule <path>` |
| Projekte | `project list`, `project create --name <n>` |
| Kristalle (v1) | `crystal list --run <id>`, `crystal show <id>` |
| Forge | `forge --lang <lang>`, `forge-fullstack`, **`forge-v2`** |
| Kristalle (v2.1) | `crystals list/show/stats/export/import` |
| Agent | `agent --intent <text>`, `agent-chat [--project <n>]` |
| Navigator | `navigate --mode <mode> --steps <n>` |
| Genesis | `genesis show`, `genesis validate` |
| Store | `store vacuum`, `store check` |
| Web-UI | `serve [--port 8420]` |

---

## `forge-v2` — Multi-Pass Render Loop

`forge-v2` ist der Hauptbefehl von v2.1. Er kombiniert die Hypercube-Dekomposition mit dem 6-Pass LLM-Enrichment zu einem vollautomatischen Code-Generierungslauf.

### Flags

| Flag | Standard | Beschreibung |
|------|----------|--------------|
| `--requirements <path>` | `examples/warehouse.toml` | TOML-Anforderungsdatei |
| `--output <dir>` | `./output-v2` | Ausgabeverzeichnis |
| `--mock-oracle` | — | Kein LLM-Aufruf (offline/CI) |
| `--trace` | — | Dekompositions-Trace ausgeben |
| `--api-key <key>` | `$OPENAI_API_KEY` | OpenAI API-Key |
| `--model <name>` | `gpt-4o-mini` | LLM-Modell |
| `--passes <n>` | `5` | Anzahl Enrichment-Pässe (1–5) |
| `--skip-pass <name>` | — | Pass überspringen (wiederholbar) |
| `--token-budget <n>` | (pro Pass) | Globales Token-Budget-Override |
| `--crystal-path <path>` | `~/.isls/crystals.json` | Pfad zur Crystal Registry |

### Die 6 Pässe

| # | Typ | Scope | Zweck |
|---|-----|-------|-------|
| 0 | Structure | All | Scaffolding aus Kristall-Templates — kein LLM |
| 1 | DomainLogic | Layer: services | Service-Implementierungen befüllen |
| 2 | EdgeCases | Layer: services | Fehlerbehandlung & Eingabevalidierung |
| 3 | Integration | All | Cross-Service-Schnittstellen harmonisieren |
| 4 | TestGeneration | Layer: tests | Unit- & Integrationstests erzeugen |
| 5 | Polish | All | Qualität, Konsistenz, Kommentare |

Jeder Pass terminiert frühzeitig wenn die Änderungsrate unter den Konvergenz-Schwellwert fällt oder das Token-Budget erschöpft ist.

### Konvergenzdiagnostik

Nach dem Lauf werden pro Pass folgende Metriken ausgegeben:

```
[Render Loop]
  Passes executed: 5
  Total tokens:    14832
  Pass 0: tokens=0,    files_modified=12, convergence=1.000
  Pass 1: tokens=8241, files_modified=8,  convergence=0.312
  Pass 2: tokens=3918, files_modified=5,  convergence=0.087
  Pass 3: tokens=1544, files_modified=3,  convergence=0.041
  Pass 4: tokens=1129, files_modified=4,  convergence=0.028
```

---

## Universal Crystal Registry

15 eingebaute Muster (Level: Universal, Konfidenz: 0.95) bilden die Wissensbasis für Pass 0 und die LLM-Prompts:

| Muster | Beschreibung |
|--------|--------------|
| `belongs_to` | Foreign-Key-Beziehung 1:1 |
| `has_many` | 1:N-Beziehung mit eager/lazy loading |
| `crud_entity` | Vollständiges Create/Read/Update/Delete |
| `state_machine` | Zustandsautomat mit Transitions-Guards |
| `pagination` | Cursor- und offset-basierte Paginierung |
| `jwt_auth` | JWT-Authentifizierung mit Refresh |
| `error_handling` | Strukturierte Fehlertypen, kein Panic |
| `soft_delete` | Logisches Löschen mit `deleted_at` |
| `audit_trail` | Unveränderliches Änderungsprotokoll |
| `event_emitter` | Pub/Sub-Ereignisbus |
| `pipeline` | Stufenweise Datenverarbeitungskette |
| `config_from_env` | Konfiguration via Umgebungsvariablen |
| `health_check` | Liveness + Readiness Endpoints |
| `rate_limiting` | Token-Bucket / Sliding-Window |
| `background_job` | Asynchrone Hintergrundaufgaben |

```bash
isls crystals list                              # Alle anzeigen
isls crystals show crud_entity                  # Detail-Ansicht
isls crystals stats                             # Nutzungsstatistiken
isls crystals export --output-file crystals.json
isls crystals import crystals.json
```

---

## Anforderungsdatei (TOML)

Eigene Projekte werden als TOML-Datei spezifiziert. Beispiel (`examples/warehouse.toml`):

```toml
[app]
name = "warehouse-system"
description = "Warehouse management with inventory, orders, reporting"

[app.modules]
inventory = "Product catalog, stock levels, reorder alerts"
orders    = "Order creation, fulfillment, cancellation, tracking"
reporting = "Daily/weekly/monthly reports, KPI dashboard"
auth      = "JWT authentication, role-based access (admin, operator)"

[backend]
language    = "rust"
framework   = "actix-web"
database    = "postgresql"
auth_method = "jwt"

[frontend]
type    = "spa"
framework = "vanilla"
styling = "minimal"

[deployment]
containerized = true
compose       = true

[constraints]
max_crates      = 1
test_coverage   = "integration"
evidence_chain  = true
```

Generierung:

```bash
isls forge-v2 --requirements examples/warehouse.toml --output ./output/warehouse --mock-oracle
```

---

## Kenndaten

| Eigenschaft | Wert |
|-------------|------|
| Crates gesamt | 42 |
| Tests | 460 (0 Fehler, 0 Warnungen) |
| Observationsrate | > 800 000 obs/s |
| Datenbanktyp | SQLite (eingebettet via rusqlite) |
| Web-UI Port | 8420 |
| Compile-Rate | 100 % |
| Konstitutionelle Constraints | 21 (ADAMANT Protocol, C4-Klasse) |
| Eingebaute Kristalle | 15 Universal |
| Render-Pässe | 6 (Structure → Polish) |
| LLM-Provider | OpenAI, Anthropic |
| Kryptographie | AES-GCM, HKDF, SHA-256 |

---

## Lizenz

MIT © Sebastian Klemm
