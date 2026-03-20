# ISLS — Intelligent Semantic Ledger Substrate

A formally verified, cryptographically anchored software intelligence platform
that generates, validates, and accumulates knowledge through adversarial testing.

## What It Does

ISLS observes data streams, discovers invariant patterns, validates them through
an 8-gate adversarial cascade, and crystallises the survivors as cryptographically
anchored, deterministically reproducible artifacts.

It also generates software from natural language, compiles and tests the output
autonomously, and accumulates validated patterns in a local knowledge base that
progressively reduces dependency on external LLMs.

## Key Numbers

| Metric | Value |
|--------|-------|
| Observation throughput | 853 K obs/sec (single laptop) |
| Crystal validation | 146 crystals, 100 % pass rate, 5 scenarios |
| Live code generation | 100 % compile rate (4 consecutive runs) |
| Oracle latency | ~800 ms (OpenAI gpt-4o-mini) |
| End-to-end fabrication | ~10 seconds |
| Test suite | 460 tests, 0 failures, 0 warnings |
| Constitutional conformance | 21/21 ADAMANT axioms, C4 class |

## Quick Start

```bash
# Build
cargo build --release

# Run full validation pipeline (scenarios + benchmarks + report)
# Linux / macOS:
./run_all_scenarios.sh
# Windows:
run_all_scenarios.bat

# The generated report opens in your browser automatically.
```

## Architecture

```
ISLS (31 crates, 460 tests)
├── Analytical Engine (C1–C18): Observe → Extract → Validate → Crystallise
├── Generative Forge  (C21–C28): Intent → PMHD → ArtifactIR → Oracle → Foundry → Crystal
├── Agent  (C30):  Natural language → Features → Architecture → Code → Compile → Test
├── Navigator (C29): TRITON spiral search + SimplexMesh + Laplacian spectral guidance
├── Swarm  (C31):  Multi-agent coordination with PMHD-backed consensus
└── Constitution:  ADAMANT Protocol (21 axioms, CC BY 4.0)
```

## ADAMANT Protocol

The constitutional governance layer. 21 machine-verifiable axioms.

- License: CC BY 4.0

## Configuration

```bash
# Set Oracle provider (auto-detects from env var)
export OPENAI_API_KEY=sk-...
# or
export ANTHROPIC_API_KEY=sk-ant-...

# Initialise
isls init

# Interactive agent
isls agent chat --project ~/my-project
```

## License

MIT — see [LICENSE](LICENSE).

## Author

Sebastian Klemm
