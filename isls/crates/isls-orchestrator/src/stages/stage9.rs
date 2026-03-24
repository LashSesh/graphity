// Stage 9: VERIFY — Full system → compilation + topology check
use std::process::Command;
use crate::{GenContext, Result};
use super::{Stage, StageResult};
use isls_reader::parse_directory;
use isls_code_topo::compute_code_topology;

pub fn run(ctx: &mut GenContext, verify_compilation: bool) -> Result<StageResult> {
    let backend_dir = ctx.output_dir.join("backend");
    let mut notes = Vec::new();
    let mut success = true;

    // 1. Topology check: parse generated backend code with Barbara
    if backend_dir.join("src").exists() {
        match parse_directory(&backend_dir.join("src")) {
            Ok(analysis) => {
                let topo = compute_code_topology(&analysis.files);
                notes.push(format!(
                    "topology: {} nodes, {} edges, layers: {}",
                    topo.node_count, topo.edge_count,
                    topo.layers.join(", ")
                ));
                notes.push(format!(
                    "functions: {}, structs: {}",
                    topo.function_signatures.len(),
                    topo.struct_names.len()
                ));
            }
            Err(e) => {
                notes.push(format!("topology parse warning: {}", e));
            }
        }
    }

    // 2. Evidence chain verification
    if ctx.evidence.is_valid() {
        notes.push(format!("evidence chain: valid ({} entries)", ctx.evidence.len()));
    } else {
        notes.push("evidence chain: INVALID".to_string());
        success = false;
    }

    // 3. Compilation check (only if requested and cargo is available)
    if verify_compilation && backend_dir.join("Cargo.toml").exists() {
        let cargo_result = Command::new("cargo")
            .args(["check", "--manifest-path"])
            .arg(backend_dir.join("Cargo.toml"))
            .output();

        match cargo_result {
            Ok(output) => {
                if output.status.success() {
                    notes.push("cargo check: PASS".to_string());
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    notes.push(format!("cargo check: FAIL\n{}", stderr.lines().take(10).collect::<Vec<_>>().join("\n")));
                    success = false;
                }
            }
            Err(e) => {
                notes.push(format!("cargo check: skipped (cargo not found: {})", e));
            }
        }
    } else {
        notes.push("cargo check: skipped (verify_compilation=false or Cargo.toml not present)".to_string());
    }

    // 4. Save evidence manifest
    let evidence_dir = ctx.output_dir.join("evidence");
    let _ = std::fs::create_dir_all(&evidence_dir);

    let manifest = serde_json::json!({
        "app_name": ctx.spec.name,
        "files_generated": ctx.files_written.len(),
        "evidence_entries": ctx.evidence.entries().len(),
        "evidence_chain_valid": ctx.evidence.is_valid(),
        "evidence_tip": ctx.evidence.tip(),
    });
    let manifest_path = evidence_dir.join("generation_manifest.json");
    let _ = std::fs::write(&manifest_path, serde_json::to_string_pretty(&manifest).unwrap_or_default());
    ctx.files_written.push(manifest_path);

    // 5. Topology report
    let topology_report = serde_json::json!({
        "message": "topology analysis complete",
        "notes": notes,
    });
    let topo_path = evidence_dir.join("topology_report.json");
    let _ = std::fs::write(&topo_path, serde_json::to_string_pretty(&topology_report).unwrap_or_default());
    ctx.files_written.push(topo_path);

    let mut result = StageResult::ok(Stage::Verify, 2, 0, 0);
    result.notes = notes;
    result.success = success;
    Ok(result)
}
