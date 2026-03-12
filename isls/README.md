# ISLS v1.0.0 — Intelligent Semantic Ledger Substrate

> **Pure-Rust Workspace · 9 Crates · Kein `unsafe` · Kein `rand` · 20/20 Acceptance Tests grün**

---

## Was ist ISLS? (Für alle verständlich)

Stell dir vor, du beobachtest einen Fluss. Das Wasser fließt ständig — mal ruhig, mal turbulent. Irgendwann erkennst du ein Muster: Nach starkem Regen steigt der Pegel immer um genau diese Menge, die Strömung dreht sich immer in diese Richtung. Du nimmst einen Stein und ritzt dieses Muster ein — **ein bleibender, unveränderlicher Beweis**, dass dieses Muster real ist.

**ISLS macht genau das — aber für beliebige Datenströme.**

### Das System in drei Sätzen

ISLS beobachtet kontinuierlich eingehende Daten (Sensormessungen, Finanzdaten, Signale — was auch immer), sucht automatisch nach stabilen, wiederkehrenden Mustern in diesen Daten und schreibt jedes gefundene Muster als **unveränderlichen „Semantischen Kristall"** in ein fälschungssicheres Hauptbuch (Ledger).

Kein Kristall kann nachträglich verändert werden. Kein Muster wird aufgezeichnet, das nicht mehrfach unabhängig verifiziert wurde. Die gesamte Herkunft jeder Aufzeichnung ist lückenlos nachvollziehbar.

---

### Das System — Schritt für Schritt erklärt

```
Rohdaten  →  Beobachten  →  Strukturieren  →  Muster finden  →  Prüfen  →  Kristall  →  Ledger
   (1)           (2)             (3)              (4)             (5)        (6)          (7)
```

#### (1) Rohdaten
Irgendwelche Eingaben: Bytes, Messwerte, Ereignisse. Das System macht keine Annahmen über das Format.

#### (2) Beobachten — `isls-observe`
Jede Eingabe wird **kanonisiert**: In eine eindeutige, standardisierte Form gebracht und mit einem kryptografischen Fingerabdruck (SHA-256-Hash) versehen. Gleiche Eingabe → immer gleicher Fingerabdruck. Das ist die Grundlage für alle späteren Beweise.

#### (3) Strukturieren — `isls-persist`
Die beobachteten Daten werden in einem **persistenten Graphen** gespeichert — einem Netzwerk aus Knoten (Datenpunkten) und Kanten (Beziehungen). Wie ein Gehirn, das Verbindungen zwischen Erlebnissen knüpft. Nichts wird je gelöscht; Knoten können nur „deaktiviert" werden. Die gesamte Geschichte bleibt erhalten.

#### (4) Muster finden — `isls-extract`
Acht mathematische Operatoren durchsuchen den Graphen nach Mustern:
- **Band**: Liegt ein Wert in einem bestimmten Bereich?
- **Ratio**: Stehen zwei Werte in einem festen Verhältnis?
- **Correlation**: Verändern sich zwei Werte gemeinsam?
- **Granger**: Lässt Wert A auf Wert B schließen?
- **Spectral**: Taucht eine bestimmte Frequenz auf?
- **Topological**: Wie ist der Graph strukturell verbunden?
- **Phase**: Sind Schwingungen synchron?
- **Contraction**: Zieht sich ein Zustandsraum zusammen?

Gefundene Muster werden zu einem **Constraint-Programm** zusammengestellt — einem geordneten Regelwerk, das den gefundenen Zustand beschreibt.

#### (5) Prüfen — `isls-consensus`
Bevor ein Muster akzeptiert wird, muss es **acht unabhängige Qualitätsgates** passieren (den sogenannten „Kairos-Moment") und einen **dualen Konsens** bestehen: Zwei vollständig unabhängige Berechnungspfade müssen zum gleichen Ergebnis kommen. Außerdem durchläuft das System eine **Proof-of-Resonance-Zustandsmaschine** (Search → Lock → Verify → Commit), die sicherstellt, dass das Muster stabil ist und nicht nur zufällig auftaucht.

#### (6) Kristall — `isls-archive`
Wenn alle Prüfungen bestanden sind, wird das Muster zu einem **Semantischen Kristall** — einem unveränderlichen Datensatz mit:
- Einem eindeutigen Inhaltshash (der Kristall *ist* sein Hash)
- Einer lückenlosen Beweiskette (welche Daten führten dazu?)
- Einem vollständigen Commit-Beweis (welche Operatoren mit welchen Versionen?)
- Topologischen Kennzahlen der Graphstruktur

#### (7) Ledger
Kristalle werden in ein append-only Archiv geschrieben. Kein Kristall kann jemals geändert oder gelöscht werden. Jede spätere Mutation am Graphen lässt bestehende Kristalle unberührt — sie sind **für immer in Stein gemeißelt**.

---

### Warum ist das nützlich?

| Anwendungsfall | Was ISLS tut |
|---|---|
| **Anomalieerkennung** | Stabile Muster werden aufgezeichnet; Abweichungen fallen auf |
| **Wissenschaftliche Reproduzierbarkeit** | Jeder Kristall ist deterministisch reproduzierbar (kein Zufall, kein `rand`) |
| **Auditierbare KI-Systeme** | Vollständige Herkunft jeder Entscheidung mit Operator-Versionierung |
| **Echtzeit-Monitoring** | Kontinuierlicher Betrieb mit konfigurierbaren Schwellwerten |
| **Unveränderliches Protokoll** | Tamper-evident Ledger für regulierte Umgebungen |

---

## Architektur

```
isls/
├── Cargo.toml                  # Workspace-Root
└── crates/
    ├── isls-types/             # C1 · Alle Datentypen, JCS-Hashing, Konfiguration
    ├── isls-observe/           # C2 · Beobachtungsadapter (Layer L0)
    ├── isls-persist/           # C3 · Persistenter HDAG-Graph (Layer L1 / MCCE)
    ├── isls-extract/           # C4 · Constraint-Extraktion (Layer L2 / ECLS)
    ├── isls-consensus/         # C5 · Konsens, PoR-Gate, Metriken (Layer L3)
    ├── isls-carrier/           # C6 · Tubus, Helix, Mandorla, Phase-Ladder
    ├── isls-archive/           # C7 · Beweisketten, Verifikation, Archiv
    ├── isls-morph/             # C8 · Morphogener Controller (Layer L4)
    └── isls-engine/            # C9 · Zustandsmaschine, Orchestrator, Tests
```

### Abhängigkeitsgraph (kein Zyklus)

```
isls-types  ←─────────────────────────────────────── (alle Crates)
    ↑
isls-observe ←── isls-persist ←── isls-extract
                      ↑                ↑
              isls-consensus ←── isls-carrier
                      ↑
              isls-archive ←── isls-morph
                      ↑
              isls-engine  (hängt von allen ab)
```

---

## Technische Kernprinzipien

| Prinzip | Umsetzung |
|---|---|
| **Kein `unsafe`** | Gesamtes Workspace ohne unsafe Rust-Code |
| **Kein `rand`** | Determinismus via `RunDescriptor`; kein Zufallszustand |
| **`BTreeMap` überall** | Statt `HashMap` — garantierte Schlüsselreihenfolge in allen deterministischen Pfaden |
| **JCS-Kanonisierung** | RFC 8785 JSON Canonicalization Scheme + SHA-256 für Content-Addressing (OI-01) |
| **Append-only** | Keine `delete`-Methoden im gesamten Workspace (Invariante I1) |
| **Operatoren versioniert** | Jeder Kristall trägt die exakten Operator-Versionen; Drift wird erkannt (Inv I20) |
| **Tri-temporales Modell** | Drei unabhängige Zeitdimensionen: `NullCenter` (vorzeit) · `IntrinsicTime` (t₂) · `CommitIndex` (t₁) |

---

## Schnellstart

```bash
# Repository klonen (Workspace liegt unter isls/)
cd isls

# Workspace bauen
cargo build --workspace --release

# Alle 20 Acceptance Tests ausführen
cargo test --workspace -- --test-threads=1

# Einzelnen Test ausführen
cargo test -p isls-engine at_03_replay_determinism

# Spezifisches Crate testen
cargo test -p isls-consensus
```

---

## Externe Abhängigkeiten

| Crate | Version | Zweck |
|---|---|---|
| `serde`, `serde_json` | 1.x | Serialisierung |
| `serde_jcs` | 0.1 | RFC 8785 JSON Canonicalization (OI-01) |
| `sha2` | 0.10 | SHA-256 Content-Addressing |
| `petgraph` | 0.6 | Gerichteter Graph (HDAG) |
| `ordered-float` | 4.x | Deterministische Float-Ordnung |
| `nalgebra` | 0.33 | Lineare Algebra (Topologie, OI-03) |
| `chrono` | 0.4 | Zeitstempel |
| `thiserror` | 2.x | Fehlertypen |
| `tracing` | 0.1 | Strukturiertes Logging |
| `tokio` | 1.x | Async-Runtime (nur `isls-engine`) |

---

## Acceptance Tests (AT-01 bis AT-20)

Alle 20 Tests befinden sich in `crates/isls-engine/tests/acceptance.rs`:

| ID | Name | Was wird geprüft? |
|---|---|---|
| AT-01 | Idempotente Ingestion | Gleiche Eingabe → gleicher Digest |
| AT-02 | Append-only | Historische Daten bleiben nach Decay abrufbar |
| AT-03 | Replay-Determinismus | Gleicher `RunDescriptor` → identische Kristall-IDs |
| AT-04 | Read-only Extraktion | `inverse_weave` verändert den Graphen nicht |
| AT-05 | Constraint-Konvergenz | Korrelierte Daten → Constraint-Programm entsteht |
| AT-06 | Provenienz-Vollständigkeit | `verify_crystal` gibt `Ok` für valide Kristalle |
| AT-07 | Schwellwert-Ablehnung | Metric unter Schwellwert → kein Kristall |
| AT-08 | Positiver Commit | Alle Gates erfüllt → Kristall im Archiv |
| AT-09 | Speicherfehler | Korruptes Warm-Tier → expliziter Fehler, Hot-Tier intakt |
| AT-10 | Nicht-Retroaktivität | Morph-Mutation ändert keine alten Kristall-Digests |
| AT-11 | Operator-Drift | Geänderte Versionsnummer → Protokollfehler erkannt |
| AT-12 | Ressourcen-Begrenzung | 100 Zyklen → Knotenanzahl innerhalb konfigurierten Limits |
| AT-13 | Dualer Konsens | Primal- und Dual-Pfad müssen beide zustimmen |
| AT-14 | PoR-Zustandsmaschine | Search → Lock → Verify → Commit Übergänge |
| AT-15 | Carrier-Migration | Friction > θ_F → Migration zugelassen |
| AT-16 | Kairos-Gate | Ein Gate unterdrückt → kein Monolith |
| AT-17 | NullCenter stateless | `NullCenter` ist Zero-Sized Unit-Struct |
| AT-18 | Tri-temporale Ordnung | n₀ ◁ t₂ ◁ t₁ in allen Commit-Traces |
| AT-19 | Content-Addressing | `crystal_id == SHA-256(JCS(Kernfelder))` |
| AT-20 | Symmetrie-Wiederherstellung | Nach Commit: Carrier zurück auf Neutral, Engine auf Idle |

---

## Invarianten (I1–I20)

Das System erzwingt 20 strukturelle Invarianten, verteilt über alle Crates:

- **I1** Append-only History (`isls-persist`: keine `delete`-Methoden)
- **I2** Kanonische Serialisierung (`isls-types`: JCS)
- **I3** Provenienz-Vollständigkeit (`isls-archive`: `verify_crystal`)
- **I4** Deterministischer Replay (`isls-engine`: `RunDescriptor`, kein `rand`)
- **I5** Read-only Extraktion (`isls-extract`: `&PersistentGraph`)
- **I6** Atomares Embedding (`isls-persist`: einzelner `apply_observations`-Aufruf)
- **I7** Phasen-Monotonie (`isls-carrier`: `delta_tau >= 0`)
- **I8** Speicher-Integrität (`isls-persist`: Digest-Prüfung bei Lesen)
- **I9** Schwellwert-gesteuerter Commit (`isls-engine`: Kairos-Gate)
- **I10** Commit-Unveränderlichkeit (`isls-archive`: kein `&mut` auf committeten Kristallen)
- **I11** Morphogene Nicht-Retroaktivität (`isls-morph`: alte Digests unverändert)
- **I12** Ressourcen-Begrenzung (`isls-engine`: konfigurierbare Limits)
- **I13** Leeres Null-Center (`isls-types`: `NullCenter` ist Unit-Struct)
- **I14** Tri-temporale Irreduzibilität (drei getrennte Zeittypen)
- **I15** Duale Helix-Kopplung (`isls-carrier`: `helix_pair` erzwingt π-Offset)
- **I16** Mandorla-Eigenfeld (`isls-carrier`: `MandorlaState ≠ TubusCoord`)
- **I17** Kein Monolith ohne Kristall (`isls-engine`: Kristall vor Commit erforderlich)
- **I18** Kein Breaking ohne Kairos (`isls-engine`: Gate-Prüfung vor Aktion)
- **I19** Rückverfolgbarkeit (`isls-archive`: `CommitProof` trägt vollständigen Trace)
- **I20** Operator-Versions-Pinning (`isls-types`: `RunDescriptor.operator_versions`)

---

## Spezifikation

Die vollständige Implementierungsspezifikation liegt im Repo-Root:

- [`ISLS_RustSpec_v1_0_0.tex`](../ISLS_RustSpec_v1_0_0.tex) — LaTeX-Quelle
- [`ISLS_RustSpec_v1_0_0.pdf`](../ISLS_RustSpec_v1_0_0.pdf) — Kompiliertes PDF

Die Spezifikation ist die **einzige normative Quelle** für alle Typen, Funktionssignaturen und Invarianten. Alle acht Open Issues (OI-01 bis OI-08) sind darin vollständig aufgelöst.

---

*ISLS v1.0.0 · Sebastian Klemm · 12. März 2026 · Rust 2021 Edition · Stable Toolchain*
