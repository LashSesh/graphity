# ISLS &mdash; Intelligent Semantic Ledger Substrate

**Version 1.0.0** &nbsp;|&nbsp; **31 Crates** &nbsp;|&nbsp; **460 Tests** &nbsp;|&nbsp; **Zero Warnings**

A formally verified, cryptographically anchored software intelligence platform that generates, validates, and accumulates knowledge through adversarial testing.

ISLS observes data streams, discovers invariant patterns, validates them through an 8-gate adversarial cascade, and crystallises the survivors as cryptographically anchored, deterministically reproducible artifacts called **Semantic Crystals**. Every crystal carries a complete evidence chain, is content-addressed (SHA-256), and can be independently verified.

The system also generates software from natural language, compiles and tests the output autonomously, and accumulates validated patterns in a local knowledge base that progressively reduces dependency on external LLMs.

Built entirely in Rust. No garbage collector. No runtime exceptions. One binary.

---

## Key Metrics

| Metric | Value |
|--------|-------|
| Observation throughput | 853 K obs/sec (single laptop) |
| Crystal validation | 146 crystals, 100% pass rate, 5 scenarios |
| Live code generation | 100% compile rate (6 consecutive runs) |
| Oracle latency | ~800 ms (OpenAI gpt-4o-mini) |
| End-to-end fabrication | ~10 seconds |
| Test suite | 460 tests, 0 failures, 0 warnings |
| Constitutional conformance | 21/21 ADAMANT axioms, C4 class |

---

## Table of Contents

- [Overview](#overview)
- [Architecture](#architecture)
- [Crate Map](#crate-map)
- [Getting Started](#getting-started)
- [CLI Reference](#cli-reference)
- [The Studio](#the-studio)
- [The Forge](#the-forge)
- [The Foundry](#the-foundry)
- [Extension Phases](#extension-phases)
- [Testing](#testing)
- [Constitution & ADAMANT](#constitution--adamant)
- [ADAMANT Protocol & Citation](#adamant-protocol--citation)
- [License](#license)
- [Author](#author)

---

## Overview

ISLS implements a five-layer processing pipeline that transforms raw observations into verified structural knowledge:

```
L0  Observe     Raw data ingestion, adapter-based, schema-validated
L1  Persist     Append-only HDAG (Hash-Directed Acyclic Graph)
L2  Extract     Constraint extraction from graph topology
L3  Consensus   Dual (primal/dual) consensus, Proof-of-Relevance gate
L4  Crystallize  Semantic Crystal emission with evidence chains
```

Every crystal carries a complete evidence chain, is content-addressed (SHA-256), and can be independently verified. The system supports deterministic replay: given the same input and descriptor, the exact same crystals are produced, bit-for-bit.

### Key Properties

- **Deterministic**: Replay-reproducible execution with saved descriptors
- **Append-only**: No mutation, no deletion, full audit trail
- **Content-addressed**: Every artifact identified by its cryptographic hash
- **Formally validated**: 21 constitutional constraints (GC-01 through GC-21)
- **Multi-scale**: Micro/meso/macro observation with bridge operators
- **Self-healing**: Carrier geometry with tubus/helix/mandorla topology

---

## Architecture

```
                    ┌─────────────────────────────────────┐
                    │         isls-cli (C11)               │
                    │   Single binary operator interface    │
                    └───────────┬─────────────────────────┘
                                │
                    ┌───────────▼─────────────────────────┐
                    │        isls-engine (C9)               │
                    │   Macro-step orchestrator             │
                    └───┬───┬───┬───┬───┬─────────────────┘
                        │   │   │   │   │
           ┌────────────┘   │   │   │   └────────────┐
           ▼                ▼   │   ▼                ▼
    isls-observe(C2)  isls-persist(C3)  isls-extract(C4)  isls-morph(C8)
       L0: Ingest      L1: HDAG     L2: Constraints    L4: Mutation
                            │
                            ▼
                    isls-consensus (C5)
                    L3: PoR Gate + Dual Consensus
                            │
              ┌─────────────┼─────────────────┐
              ▼             ▼                  ▼
       isls-archive(C7) isls-carrier(C6) isls-capsule(C14)
       Evidence chains  Tubus geometry   Secret release
                            │
              ┌─────────────┼─────────────┐
              ▼             ▼             ▼
       isls-forge(C23) isls-foundry(C27) isls-gateway(C19)
       Generative       Fabrication      REST + WebSocket
       Synthesis        Pipeline         + Studio UI
```

---

## Crate Map

ISLS is organized as a Cargo workspace with 31 crates. Each crate has a designation (C1&ndash;C31) and a clear responsibility boundary.

### Core Pipeline

| Crate | ID | Description |
|---|---|---|
| `isls-types` | C1 | Canonical data model, serialization, hashing. Zero internal dependencies. |
| `isls-observe` | C2 | Observation adapters (L0): raw data ingestion, schema validation |
| `isls-persist` | C3 | Persistent HDAG engine (L1): append-only hash-directed acyclic graph |
| `isls-extract` | C4 | Constraint extraction (L2): pattern mining from graph topology |
| `isls-consensus` | C5 | Consensus protocol (L3): dual primal/dual, Proof-of-Relevance gate |
| `isls-carrier` | C6 | Carrier geometry: tubus, helix pair, mandorla, phase-ladder |
| `isls-archive` | C7 | Evidence chains, replay verification (L3/L4) |
| `isls-morph` | C8 | Morphogenic controller (L4): crystal mutation and evolution |
| `isls-engine` | C9 | State machine orchestrator: macro-step loop, global state |

### Infrastructure

| Crate | ID | Description |
|---|---|---|
| `isls-harness` | C10 | Validation harness: benchmarks, validators, metric collectors, reporters |
| `isls-cli` | C11 | Single-binary operator interface: all commands, dashboard, config |
| `isls-registry` | C12 | Digest-bound catalog for operators, profiles, obligations, macros |
| `isls-manifest` | C13 | Execution manifest: run-level content-addressed meta-artifact |
| `isls-capsule` | C14 | Capsule protocol: evidence-bound secret release (OLP) |
| `isls-scheduler` | C15 | Spiral scheduler: adaptive tick granularity |
| `isls-topology` | C16 | Topological-spectral orbit core: spectral analysis, CTQW, Kuramoto, DTL |
| `isls-store` | C17 | SQLite-backed persistence: projects, runs, crystals, settings |
| `isls-scale` | C18 | Hierarchical multi-scale observation: hypercube universes, bridges, ladders |

### Generative Synthesis

| Crate | ID | Description |
|---|---|---|
| `isls-gateway` | C19 | REST + WebSocket API, Studio web interface |
| `isls-pmhd` | C21 | Polycentric Multi-Hypothesis Drill: adversarial hypothesis testing |
| `isls-artifact-ir` | C22 | Universal Artifact Intermediate Representation |
| `isls-forge` | C23 | Generative Synthesis Engine: spec &rarr; drill &rarr; IR &rarr; crystal |
| `isls-compose` | C24 | Recursive decomposition and hierarchical composition |
| `isls-oracle` | C25 | Hybrid Synthesis Oracle: memory-first &rarr; LLM fallback &rarr; skeleton |
| `isls-templates` | C26 | Crystallized Architecture Pattern Catalog: pre-validated skeletal templates |
| `isls-foundry` | C27 | Closed-loop software fabrication: compile-test-fix pipeline |
| `isls-multilang` | C28 | Babylon Bridge: IR-validated multi-language code generation |
| `isls-navigator` | C29 | Spectral-Guided Pattern Space Explorer: TRITON spirals + Laplacian mesh |
| `isls-agent` | C30 | Autonomous Goal-Directed Agent: plan &rarr; step &rarr; adapt &rarr; complete |
| `isls-swarm` | C31 | Multi-Agent Swarm Coordinator: seed-diverse agents + resonance consensus |

---

## Getting Started

### Prerequisites

- **Rust** 1.75+ (stable)
- **SQLite** (bundled via `rusqlite`, no system install needed)

### Build

```bash
cd isls
cargo build --release
```

The single binary is at `target/release/isls`.

### Initialize

```bash
isls init
```

Creates `~/.isls/` with default config, data directories, and Genesis Crystal.

### Quick Run

```bash
# Run full validation pipeline (scenarios + benchmarks + report)
# Linux / macOS:
./run_all_scenarios.sh
# Windows:
run_all_scenarios.bat

# The generated report opens in your browser automatically.
```

Or step by step:

```bash
# Ingest synthetic data (500 entities)
isls ingest --adapter synthetic --entities 500

# Run the macro-step loop (50 ticks)
isls run --ticks 50

# Check system health
isls status

# Generate HTML report
isls report --full-html
```

### Start the Studio

```bash
isls serve
```

Opens the Gateway at `http://localhost:8420/studio` &mdash; a real-time web interface with seven views for full system control.

---

## CLI Reference

```
USAGE:
  isls <COMMAND> [OPTIONS]

COMMANDS:
  init                           Generate default config + data dirs
  ingest [options]               Attach a data source
    --adapter <name>             synthetic, file-csv, file-jsonl, replay
    --path <path>                Data path (for file adapters)
    --entities <n>               Entity count (for synthetic)
    --scenario <name>            Scenario: basic, regime, causal, break, scale
  run [options]                  Start the macro-step loop
    --replay <descriptor>        Deterministic replay
    --mode <shadow|live>         Operation mode (default: live)
    --ticks <n>                  Number of ticks (default: 10)
  execute [options]              Execute a crystal in generative mode
    --input <path|latest>        Crystal file or 'latest'
    --ticks <n>                  Ticks to execute
    --output <dir>               Output directory
  seal --secret <text>           Seal a secret under a manifest-bound capsule
  open --capsule <path>          Decrypt a capsule
  bench                          Full benchmark suite
  validate [--formal] [--retro]  Run validation suites
  report [--json|--html|--full-html]  Health dashboard
  status                         One-line system health summary
  serve [--port 8420]            Start Gateway + Studio web interface

PROJECT & CRYSTAL COMMANDS:
  project list                   List projects
  project create --name <n>      Create project
  crystal list --run <id>        List crystals for a run
  crystal show <id>              Show crystal detail
  export --run <id>              Export run data

GENESIS COMMANDS:
  genesis show                   Display Genesis Crystal
  genesis validate               Validate constitutional constraints

ORACLE COMMANDS (C25):
  oracle status                  Oracle provider status
  oracle memory                  Pattern memory browser
  oracle seal-key --key <k>      Seal API key

TEMPLATE COMMANDS (C26):
  template list                  List available templates
  template show <name>           Show template structure
  template create --name <n>     Create from structure file
  template distill --crystal <id>  Distill from forge result
  template compose --name <n>    Compose templates

GATEWAY COMMANDS (C19):
  serve [--port <port>]          Start Gateway + Studio (default: 8420)
```

---

## The Studio

The Studio is a single-page web application served by the Gateway at `http://localhost:8420/studio`. It provides a unified operator interface for the entire substrate.

**Design principles**: Dark theme, keyboard-first, real-time via WebSocket, zero external dependencies (no npm, no React, no build step &mdash; pure vanilla HTML/JS/CSS in a single file embedded in the binary).

### Seven Views

| # | View | Shortcut | Description |
|---|---|---|---|
| 1 | **Dashboard** | `Ctrl+1` | System health at a glance: metrics, scenarios, live event feed |
| 2 | **Forge** | `Ctrl+2` | Generative interface: describe intent, watch atoms build |
| 3 | **Explorer** | `Ctrl+3` | Browse crystals, manifests, patterns, templates |
| 4 | **Monitor** | `Ctrl+4` | Live engine telemetry: canvas charts, event stream |
| 5 | **Foundry** | `Ctrl+5` | Project fabrication with compile/test feedback |
| 6 | **Oracle** | `Ctrl+6` | Autonomy metrics, pattern memory, budget status |
| 7 | **Constitution** | `Ctrl+7` | Genesis Crystal, ADAMANT conformance (GC-01 to GC-21) |

### Keyboard Shortcuts

| Shortcut | Action |
|---|---|
| `Ctrl+1..7` | Switch views |
| `Ctrl+K` | Command palette (fuzzy search) |
| `Ctrl+Enter` | Execute primary action (Forge/Build) |
| `Ctrl+S` | Start/stop engine |
| `Ctrl+E` | Open Explorer |
| `Escape` | Close modal |
| `/` | Focus search field |

### API Endpoints

```
GET  /studio                    Serve the Studio
GET  /health                    System health JSON
GET  /status                    Engine status
GET  /api/dashboard             Aggregated dashboard data
GET  /metrics                   Current metrics
GET  /crystals?limit=N&scale=S  List crystals
GET  /crystals/{id}             Crystal detail
POST /forge                     Start forge synthesis
GET  /api/forge/progress        Forge progress (polling)
POST /api/foundry/fabricate     Start fabrication
GET  /api/foundry/progress      Fabrication progress
GET  /api/foundry/files/{id}    Generated file content
GET  /api/foundry/download/{id} Download project
POST /api/command               Execute command palette action
POST /engine/start|stop|step    Engine control
WS   /events                    Real-time event stream
```

---

## The Forge

The Forge (C23) is the generative synthesis engine. Given a `DecisionSpec` (natural language intent + constraints), it:

1. **Drills** through the PMHD (C21) to produce monoliths
2. **Translates** monoliths into an Artifact IR (C22)
3. **Interprets** through a domain-specific Matrix (Rust, TypeScript, etc.)
4. **Synthesizes** concrete output using the Oracle (C25)
5. **Evaluates** quality and stores patterns for reuse
6. **Emits** the result as a Semantic Crystal with full provenance

The Oracle operates on a memory-first principle: known patterns are reused from the pattern memory before falling back to LLM generation. This enables progressive autonomy &mdash; the system becomes more self-sufficient over time.

---

## The Foundry

The Foundry (C27) is the closed-loop software fabrication pipeline. It takes a Forge output and:

1. **Writes** files to a project directory
2. **Runs** `cargo check` (or language-appropriate compiler)
3. **If errors**: sends diagnostics back to the Oracle for correction
4. **Retries** up to 5 attempts (compile &rarr; fix &rarr; compile)
5. **Runs** `cargo test` and `cargo clippy` on success
6. **Reports** full fabrication status with file tree

The Foundry turns the Forge's generative output into compiling, tested, lint-clean code.

---

## Extension Phases

The ISLS core (Phases 1&ndash;6) defines the fundamental substrate. Extensions add capabilities:

| Phase | Version | Crates | Title |
|---|---|---|---|
| 1&ndash;6 | v1.0.0 | C1&ndash;C18 | Core substrate: types, pipeline, topology, scale, store |
| 7 | v1.0.0 | C26 | Crystallized Architecture Pattern Catalog |
| 8 | v1.0.0 | C27 | The Foundry &mdash; Closed-Loop Software Fabrication |
| 9 | v1.0.0 | C19 | The Studio &mdash; Unified Web-Based Operator Interface |
| 10 | v1.0.0 | C28 | Babylon Bridge &mdash; IR-Validated Multi-Language Forge |
| 11 | v1.0.0 | C29 | Navigator &mdash; Spectral-Guided Pattern Space Explorer |
| 12 | v1.0.0 | C30 | Agent &mdash; Autonomous Goal-Directed Agent |
| 13 | v1.0.0 | C31 | Swarm &mdash; Multi-Agent Swarm Coordinator |

---

## Testing

```bash
# Run all tests
cargo test --workspace

# Run a specific crate's tests
cargo test -p isls-gateway
cargo test -p isls-forge

# Run benchmarks
isls bench

# Run formal validation
isls validate --formal

# Run retrospective validation
isls validate --retro
```

### Test Scenarios

ISLS ships with five validation scenarios:

| Scenario | Description | Crystals |
|---|---|---|
| **S-Basic** | Standard entity correlation discovery | 51 |
| **S-Regime** | Regime change detection and adaptation | 22 |
| **S-Causal** | Causal structure inference | 16 |
| **S-Break** | Structural break identification | 21 |
| **S-Scale** | Multi-scale hierarchy navigation | 36 |

All scenarios target 100% pass rate with 8 formal checks per crystal.

### Metrics (M1&ndash;M24)

The system tracks 24 metrics across five categories:

- **Layer Health** (M1&ndash;M5): Ingestion, graph growth, constraints, crystal rate, mutation
- **Core Quality** (M6&ndash;M14): Replay fidelity, convergence, stability, gate selectivity, consensus, PoR latency, evidence integrity, version drift, storage efficiency
- **Performance** (M15&ndash;M19): Macro-step latency, memory footprint, extraction throughput, archive growth, carrier migration
- **Empirical Domain** (M20&ndash;M24): Constraint hit rate, crystal predictive value, signal lead time, basket quality lift, coverage growth

---

## Constitution & ADAMANT

Every ISLS instance is anchored to a **Genesis Crystal** &mdash; a root-of-trust that encodes the ADAMANT specification (v1.0.0). The system enforces 21 constitutional constraints:

| ID | Source | Constraint |
|---|---|---|
| GC-01 | Axiom 2.0.1 | State Boundedness |
| GC-02 | Axiom 2.0.2 | Typed Operations |
| GC-03 | Axiom 2.0.3 | Trace Discipline |
| GC-04 | Axiom 2.0.4 | Content Addressing |
| GC-05 | Axiom 2.0.5 | Acyclicity |
| GC-06 | Sec 5 | Observation Integrity |
| GC-07 | Sec 6 | Persistence Guarantee |
| GC-08 | Sec 7 | Constraint Soundness |
| GC-09 | Sec 8 | Crystal Validity |
| GC-10 | Sec 9 | Consensus Protocol |
| GC-11 | Sec 10 | Topology Invariants |
| GC-12 | Sec 11 | Carrier Geometry |
| GC-13 | Sec 12 | Multi-Scale Coherence |
| GC-14 | Sec 13 | Archive Immutability |
| GC-15 | Sec 14 | Scheduler Fairness |
| GC-16 | Sec 15 | Capsule Security |
| GC-17 | Sec 16 | Store Integrity |
| GC-18 | Sec 17 | Manifest Completeness |
| GC-19 | Sec 18 | Gateway Conformance |
| GC-20 | Sec 19 | Forge Determinism |
| GC-21 | Sec 20 | Human Override |

Conformance class **C4 (Constitutional)** means all 21 constraints pass with zero drift.

```bash
# Verify constitutional conformance
isls genesis validate
```

---

## Project Structure

```
graphity/
  isls/
    Cargo.toml              # Workspace root (31 crates)
    Cargo.lock
    crates/
      isls-types/           # C1: Core data model
      isls-observe/         # C2: Observation adapters (L0)
      isls-persist/         # C3: Persistent HDAG (L1)
      isls-extract/         # C4: Constraint extraction (L2)
      isls-consensus/       # C5: Consensus protocol (L3)
      isls-carrier/         # C6: Carrier geometry
      isls-archive/         # C7: Evidence chains
      isls-morph/           # C8: Morphogenic controller (L4)
      isls-engine/          # C9: State machine orchestrator
      isls-harness/         # C10: Validation harness
      isls-cli/             # C11: Operator CLI
      isls-registry/        # C12: Digest-bound catalog
      isls-manifest/        # C13: Execution manifest
      isls-capsule/         # C14: Capsule protocol
      isls-scheduler/       # C15: Spiral scheduler
      isls-topology/        # C16: Spectral-topological core
      isls-store/           # C17: SQLite persistence
      isls-scale/           # C18: Multi-scale observation
      isls-gateway/         # C19: REST + WebSocket + Studio
      isls-pmhd/            # C21: Hypothesis drill
      isls-artifact-ir/     # C22: Artifact IR
      isls-forge/           # C23: Generative synthesis
      isls-compose/         # C24: Decomposition & composition
      isls-oracle/          # C25: Hybrid synthesis oracle
      isls-templates/       # C26: Architecture templates
      isls-foundry/         # C27: Fabrication pipeline
  LICENSE                   # MIT
```

---

## ADAMANT Protocol & Citation

ISLS is governed by the **ADAMANT Protocol** &mdash; 21 machine-verifiable constitutional axioms (CC BY 4.0).

If you use ISLS in academic work, please cite:

> Klemm, S. (2026). *ISLS &mdash; Intelligent Semantic Ledger Substrate* (Version 1.0.0). Zenodo. [https://doi.org/10.5281/zenodo.XXXXXXX](https://doi.org/10.5281/zenodo.XXXXXXX)

*(Zenodo DOI will be assigned upon first release.)*

---

## License

MIT &mdash; see [LICENSE](LICENSE).

---

## Author

Sebastian Klemm
