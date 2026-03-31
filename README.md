# ISLS — Intelligent Semantic Ledger Substrate

Formally verified, cryptographically anchored software intelligence platform. ISLS observes data streams, discovers invariant patterns, validates them through adversarial testing, and generates full-stack applications from natural language.

One sentence. One app. Zero manual steps.

## Quick Start

```bash
# Build
cargo build --release

# Generate an app from natural language (requires OpenAI API key)
isls forge-chat -m "Hotel management with rooms, guests, and bookings" \
    --api-key $OPENAI_API_KEY --output ./output/hotel

# Generate from a TOML spec (offline, no LLM)
isls forge-v2 --requirements examples/warehouse.toml --mock-oracle --output ./output/warehouse

# Inspect the norm catalog
isls norms list
isls norms stats

# Start the Studio web UI
isls serve --port 8420
```

## Architecture

```
Chat/TOML --> isls-chat ---------> isls-forge-llm (S0-S7) --> Output
                |                       |      ^                 |
                |   norm-match          |      |                 |
                +-- NormRegistry <------+------+----- observe ---+
                        |                      |
                        +-- ~/.isls/norms.json -+
```

The pipeline is a closed loop (D4): generated artifacts feed back into the norm system, and recurring patterns across domains are auto-promoted to first-class norms that enrich future generations.

### Pipeline Stages

| Stage | Name | Description |
|-------|------|-------------|
| S0 | Ingest | AppSpec loaded from TOML or chat extraction |
| S1 | Canon | Entity names canonicalized to snake_case |
| S2 | Expand | Codegen-HDAG built deterministically from AppSpec |
| S4 | Solve | Topological traversal — structural + LLM nodes |
| S5 | Gate | Single `cargo check` after complete generation |
| S6 | Coagula | Anomaly path — up to 3 fix cycles on compile failure |
| S7 | Emit | Project directory written |
| S7.1 | Observe | Artifacts collected and fed to norm learning (D4) |

## Crates

| Crate | Purpose |
|-------|---------|
| **isls-types** | Core data model — crystals, observations, 5D state, carrier geometry |
| **isls-norms** | Self-defining norm system — composable full-stack patterns, auto-discovery |
| **isls-reader** | Multi-language source code parser (Rust, TypeScript, Python, Go) |
| **isls-code-topo** | Code topology computation and structural analysis |
| **isls-hypercube** | Requirement-space decomposition via spectral analysis |
| **isls-forge-llm** | LLM-driven code generation engine with HDAG pipeline |
| **isls-chat** | Chat-driven intent recognition and norm-guided enrichment |
| **isls-agent** | Autonomous goal-directed agent (plan, step, adapt) |
| **isls-cli** | Single-binary operator interface (`isls`) |
| **isls-gateway** | REST + WebSocket API with embedded Studio web UI |

## CLI Commands

```
isls forge-v2     HDAG code generation pipeline (Staged Closure)
isls forge-chat   Natural language to compiled application
isls norms        Inspect norm catalog, candidates, and auto-discovered norms
isls serve        Start the Gateway / Studio web interface
isls help         Print usage information
```

### Norms Subcommands

```
isls norms list [--auto-only]     List all norms (builtin + auto-discovered)
isls norms inspect <norm-id>      Show full norm details
isls norms candidates             List candidate pool
isls norms stats                  Summary statistics
isls norms reset                  Delete ~/.isls/norms.json
```

## Norm System

ISLS ships with 24+ builtin norms (CRUD-Entity, JWT-Auth, Pagination, Inventory, etc.) and automatically discovers new norms from generation runs:

1. Each generation run produces artifacts across all layers
2. The artifact collector extracts cross-layer patterns
3. Patterns observed across 3+ domains with 85%+ consistency are promoted
4. Promoted norms enrich future generations with structural priors

Auto-discovered norms are persisted to `~/.isls/norms.json`.

### Promotion Criteria

| Criterion | Threshold |
|-----------|-----------|
| Consistency (Jaccard) | >= 0.85 |
| Distinct domains | >= 3 |
| Consistent layers | >= 4 |
| Total observations | >= 5 |
| Artifact presence | >= 80% |

## Testing

```bash
cargo test --workspace          # Full test suite (171 tests)
cargo test -p isls-norms        # Norm system tests
cargo test -p isls-forge-llm    # Generation pipeline tests
cargo test -p isls-chat         # Chat + enrichment tests
cargo test -p isls-cli          # CLI tests
```

## Specifications

- `isls_d4_spec.tex` — D4 Ophan-Orbits: Auto-Norm Emergence (current)

## License

MIT
