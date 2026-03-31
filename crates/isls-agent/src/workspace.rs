// isls-agent: workspace.rs — C30 Workspace Awareness
//
// Analyzes a Rust project directory using line-based string matching (no
// full Rust parser required).  ~80% accuracy is the goal; the Oracle fills
// the remaining gaps from actual file contents included in the prompt.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

// ─── Error ────────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct WorkspaceError(pub String);

impl std::fmt::Display for WorkspaceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "workspace error: {}", self.0)
    }
}

impl std::error::Error for WorkspaceError {}

// ─── CrateType ────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[derive(Default)]
pub enum CrateType {
    Bin,
    #[default]
    Lib,
    Workspace,
}


// ─── ModuleInfo ───────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModuleInfo {
    /// Relative path from workspace root, e.g. "src/database.rs"
    pub path: String,
    /// Collected public items: "pub fn list_bookmarks", "pub struct Bookmark"
    pub public_items: Vec<String>,
    /// Lines of code
    pub loc: usize,
}

// ─── TypeInfo ─────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TypeInfo {
    pub name: String,
    /// "struct" or "enum"
    pub kind: String,
    /// Best-effort field list (may be empty for complex generics)
    pub fields: Vec<String>,
    pub derives: Vec<String>,
    pub file: String,
}

// ─── FunctionInfo ─────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FunctionInfo {
    pub name: String,
    /// Full signature line, e.g. `pub async fn get_user(id: i64) -> Option<User>`
    pub signature: String,
    pub is_public: bool,
    pub has_test: bool,
    pub file: String,
}

// ─── RouteInfo ────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RouteInfo {
    /// HTTP method (GET, POST, PUT, DELETE, PATCH, ROUTE)
    pub method: String,
    /// Path literal found in source, e.g. "/api/bookmarks"
    pub path: String,
    pub file: String,
}

// ─── AgentWorkspace ───────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentWorkspace {
    pub root: PathBuf,
    pub crate_type: CrateType,
    pub modules: Vec<ModuleInfo>,
    pub types: Vec<TypeInfo>,
    pub functions: Vec<FunctionInfo>,
    pub routes: Vec<RouteInfo>,
    /// Dependency names from Cargo.toml `[dependencies]` section
    pub dependencies: Vec<String>,
    pub file_count: usize,
    pub loc: usize,
}

impl AgentWorkspace {
    // ─── Public API ─────────────────────────────────────────────────────────

    /// Analyze a project at `root`.  Reads Cargo.toml + all *.rs under src/.
    pub fn analyze(root: &Path) -> Result<Self, WorkspaceError> {
        let cargo_path = root.join("Cargo.toml");
        let cargo_src = std::fs::read_to_string(&cargo_path).map_err(|e| {
            WorkspaceError(format!("cannot read Cargo.toml at {}: {}", cargo_path.display(), e))
        })?;

        let crate_type = detect_crate_type(&cargo_src);
        let dependencies = parse_dependencies(&cargo_src);

        let mut modules: Vec<ModuleInfo> = Vec::new();
        let mut types: Vec<TypeInfo> = Vec::new();
        let mut functions: Vec<FunctionInfo> = Vec::new();
        let mut routes: Vec<RouteInfo> = Vec::new();
        let mut total_loc: usize = 0;
        let mut file_count: usize = 0;

        let src_dir = root.join("src");
        if src_dir.exists() {
            collect_files(&src_dir, root, &mut modules, &mut types, &mut functions, &mut routes, &mut total_loc, &mut file_count);
        }

        Ok(AgentWorkspace {
            root: root.to_path_buf(),
            crate_type,
            modules,
            types,
            functions,
            routes,
            dependencies,
            file_count,
            loc: total_loc,
        })
    }

    /// Re-analyze the project in place.
    pub fn refresh(&mut self) -> Result<(), WorkspaceError> {
        let fresh = Self::analyze(&self.root)?;
        *self = fresh;
        Ok(())
    }

    /// Find modules whose names/items overlap with keywords in `task`.
    ///
    /// Scoring: each word longer than 3 chars in `task` is matched against
    /// the module path and all its public items (case-insensitive).
    pub fn relevant_files<'a>(&'a self, task: &str) -> Vec<&'a ModuleInfo> {
        let keywords: Vec<String> = task
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.len() > 3)
            .map(|w| w.to_lowercase())
            .collect();

        if keywords.is_empty() {
            return self.modules.iter().collect();
        }

        let mut scored: Vec<(usize, &ModuleInfo)> = self
            .modules
            .iter()
            .map(|m| {
                let path_lc = m.path.to_lowercase();
                let items_lc: String = m
                    .public_items
                    .iter()
                    .map(|s| s.to_lowercase())
                    .collect::<Vec<_>>()
                    .join(" ");
                let haystack = format!("{} {}", path_lc, items_lc);

                let score = keywords.iter().filter(|kw| {
                    // Exact match
                    if haystack.contains(kw.as_str()) { return true; }
                    // 6-char stem match for longer keywords (handles authenticate vs authentication)
                    if kw.len() >= 6 {
                        let stem = &kw[..6];
                        if haystack.contains(stem) { return true; }
                    }
                    false
                }).count();
                (score, m)
            })
            .filter(|(score, _)| *score > 0)
            .collect();

        scored.sort_by(|a, b| b.0.cmp(&a.0));
        scored.into_iter().map(|(_, m)| m).collect()
    }

    /// Build a context string for Oracle prompts.
    ///
    /// For each relevant module, reads the file from disk, smart-truncates
    /// to ≤ `max_file_chars`, and wraps in a markdown-ish block.
    /// Total output is capped at `max_chars`.
    pub fn context_for_prompt(&self, relevant: &[&ModuleInfo], max_chars: usize) -> String {
        let mut parts: Vec<String> = Vec::new();
        let mut used: usize = 0;

        for m in relevant {
            let full_path = self.root.join(&m.path);
            let content = std::fs::read_to_string(&full_path).unwrap_or_default();
            let truncated = smart_truncate(&content, 2000);
            let chunk = format!("FILE: {}\n```rust\n{}\n```", m.path, truncated);
            if used + chunk.len() > max_chars {
                break;
            }
            used += chunk.len();
            parts.push(chunk);
        }

        parts.join("\n\n")
    }

    /// Short one-line summary.
    pub fn summary(&self) -> String {
        format!(
            "Rust {:?}, {} module(s), {} type(s), {} fn(s), {} LOC",
            self.crate_type,
            self.modules.len(),
            self.types.len(),
            self.functions.len(),
            self.loc,
        )
    }
}

// ─── Internal Parsers ─────────────────────────────────────────────────────────

fn detect_crate_type(cargo_toml: &str) -> CrateType {
    // Workspace if it has a [workspace] section
    if cargo_toml.contains("[workspace]") {
        return CrateType::Workspace;
    }
    // Bin if there's a [[bin]] section or src/main.rs reference or is_bin marker
    if cargo_toml.contains("[[bin]]") || cargo_toml.contains("src/main.rs") {
        return CrateType::Bin;
    }
    CrateType::Lib
}

fn parse_dependencies(cargo_toml: &str) -> Vec<String> {
    let mut deps: Vec<String> = Vec::new();
    let mut in_deps = false;

    for line in cargo_toml.lines() {
        let t = line.trim();
        if t == "[dependencies]" || t == "[dev-dependencies]" {
            in_deps = true;
            continue;
        }
        if t.starts_with('[') {
            in_deps = false;
        }
        if in_deps && !t.is_empty() && !t.starts_with('#') {
            if let Some(name) = t.split('=').next() {
                let name = name.trim().to_string();
                if !name.is_empty() {
                    deps.push(name);
                }
            }
        }
    }
    deps
}

/// Recursively walk `dir`, parse each .rs file, accumulate results.
#[allow(clippy::too_many_arguments)]
fn collect_files(
    dir: &Path,
    root: &Path,
    modules: &mut Vec<ModuleInfo>,
    types: &mut Vec<TypeInfo>,
    functions: &mut Vec<FunctionInfo>,
    routes: &mut Vec<RouteInfo>,
    total_loc: &mut usize,
    file_count: &mut usize,
) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, root, modules, types, functions, routes, total_loc, file_count);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            let rel = path.strip_prefix(root).unwrap_or(&path);
            let rel_str = rel.to_string_lossy().replace('\\', "/");
            let content = match std::fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let loc = content.lines().count();
            *total_loc += loc;
            *file_count += 1;

            let (mut t_types, mut t_fns, mut t_routes, pub_items) =
                parse_file(&rel_str, &content);

            modules.push(ModuleInfo {
                path: rel_str.clone(),
                public_items: pub_items,
                loc,
            });
            types.append(&mut t_types);
            functions.append(&mut t_fns);
            routes.append(&mut t_routes);
        }
    }
}

/// Parse a single file's content into types, functions, and routes.
/// Returns (types, functions, routes, public_items_for_module).
fn parse_file(
    file_path: &str,
    content: &str,
) -> (Vec<TypeInfo>, Vec<FunctionInfo>, Vec<RouteInfo>, Vec<String>) {
    let mut types: Vec<TypeInfo> = Vec::new();
    let mut functions: Vec<FunctionInfo> = Vec::new();
    let mut routes: Vec<RouteInfo> = Vec::new();
    let mut pub_items: Vec<String> = Vec::new();

    let mut pending_derives: Vec<String> = Vec::new();
    let mut in_test_block = false;

    // Simple depth tracker for #[cfg(test)] block detection
    let mut test_brace_depth: i32 = 0;

    // State for multi-line struct body collection
    let mut current_type: Option<(String, String, Vec<String>)> = None; // (name, kind, derives)
    let mut current_fields: Vec<String> = Vec::new();
    let mut struct_brace_depth: i32 = 0;

    for line in content.lines() {
        let t = line.trim();

        // ── test block detection ───────────────────────────────────────────
        if t == "#[cfg(test)]" {
            in_test_block = true;
        }
        if in_test_block {
            for c in t.chars() {
                if c == '{' { test_brace_depth += 1; }
                if c == '}' { test_brace_depth -= 1; }
            }
            if test_brace_depth <= 0 {
                in_test_block = false;
                test_brace_depth = 0;
            }
            // Still parse fn signatures inside tests for has_test detection
        }

        // ── Struct/enum body tracking ──────────────────────────────────────
        if let Some((ref name, ref kind, ref derives)) = current_type.clone() {
            // Count braces in this line
            for c in t.chars() {
                if c == '{' { struct_brace_depth += 1; }
                if c == '}' { struct_brace_depth -= 1; }
            }
            // If we're at depth 1, this line may be a field
            if struct_brace_depth == 1 && !t.is_empty() && !t.starts_with("//")
                && !t.starts_with("pub(") && !t.starts_with("//!") {
                let field_candidate = t.trim_end_matches(',').to_string();
                if !field_candidate.is_empty() && !field_candidate.starts_with("//") {
                    current_fields.push(field_candidate);
                }
            }
            if struct_brace_depth <= 0 {
                types.push(TypeInfo {
                    name: name.clone(),
                    kind: kind.clone(),
                    fields: current_fields.clone(),
                    derives: derives.clone(),
                    file: file_path.to_string(),
                });
                if kind == "struct" {
                    pub_items.push(format!("pub struct {}", name));
                } else {
                    pub_items.push(format!("pub enum {}", name));
                }
                current_type = None;
                current_fields.clear();
                struct_brace_depth = 0;
            }
            continue;
        }

        // ── Derive attributes ──────────────────────────────────────────────
        if t.starts_with("#[derive(") || (t.starts_with("#[") && t.contains("derive(")) {
            pending_derives = extract_derives(t);
            continue;
        }

        // Clear pending derives on non-attribute, non-comment lines
        if !t.starts_with('#') && !t.starts_with("//") && !t.is_empty() {
            // We check for type/fn first, then clear
        } else if !t.starts_with("#[derive") && t.starts_with('#') {
            pending_derives.clear();
        }

        // ── Struct detection ───────────────────────────────────────────────
        if t.starts_with("pub struct ") || t.starts_with("pub(crate) struct ") {
            let after = if let Some(rest) = t.strip_prefix("pub struct ") {
                rest
            } else if let Some(rest) = t.strip_prefix("pub(crate) struct ") {
                rest
            } else {
                unreachable!()
            };
            let name = ident_head(after);
            if !name.is_empty() {
                let derives: Vec<String> = std::mem::take(&mut pending_derives);
                struct_brace_depth = 0;
                let mut is_unit_struct = false;
                let mut opened_brace = false;
                for c in t.chars() {
                    if c == '{' { struct_brace_depth += 1; opened_brace = true; }
                    if c == '}' { struct_brace_depth -= 1; }
                    if c == ';' && !opened_brace {
                        // Tuple struct or unit struct (no braces at all)
                        types.push(TypeInfo {
                            name: name.clone(),
                            kind: "struct".to_string(),
                            fields: vec![],
                            derives: derives.clone(),
                            file: file_path.to_string(),
                        });
                        pub_items.push(format!("pub struct {}", name));
                        is_unit_struct = true;
                        break;
                    }
                }
                if !is_unit_struct {
                    if struct_brace_depth > 0 {
                        // Multi-line: body continues on next lines
                        current_type = Some((name, "struct".to_string(), derives));
                        current_fields.clear();
                    } else if opened_brace {
                        // Inline single-line struct: { fields } on same line
                        types.push(TypeInfo {
                            name: name.clone(),
                            kind: "struct".to_string(),
                            fields: vec![],
                            derives,
                            file: file_path.to_string(),
                        });
                        pub_items.push(format!("pub struct {}", name));
                    }
                }
            }
            pending_derives.clear();
            continue;
        }

        // ── Enum detection ─────────────────────────────────────────────────
        if t.starts_with("pub enum ") || t.starts_with("pub(crate) enum ") {
            let after = if let Some(rest) = t.strip_prefix("pub enum ") {
                rest
            } else if let Some(rest) = t.strip_prefix("pub(crate) enum ") {
                rest
            } else {
                unreachable!()
            };
            let name = ident_head(after);
            if !name.is_empty() {
                let derives = std::mem::take(&mut pending_derives);
                struct_brace_depth = 0;
                for c in t.chars() {
                    if c == '{' { struct_brace_depth += 1; }
                    if c == '}' { struct_brace_depth -= 1; }
                }
                if struct_brace_depth > 0 {
                    current_type = Some((name, "enum".to_string(), derives));
                    current_fields.clear();
                } else {
                    types.push(TypeInfo {
                        name: name.clone(),
                        kind: "enum".to_string(),
                        fields: vec![],
                        derives,
                        file: file_path.to_string(),
                    });
                    pub_items.push(format!("pub enum {}", name));
                }
            }
            pending_derives.clear();
            continue;
        }

        pending_derives.clear();

        // ── Function detection ─────────────────────────────────────────────
        let is_pub_fn = t.starts_with("pub fn ") || t.starts_with("pub async fn ");
        let is_any_fn = is_pub_fn || t.starts_with("fn ") || t.starts_with("async fn ");

        if is_any_fn {
            let (fn_name, signature) = extract_fn(t);
            if !fn_name.is_empty() {
                let has_test = in_test_block
                    || content.contains(&format!("#[test]\n    fn {}", fn_name))
                    || content.contains(&format!("#[test]\n    async fn {}", fn_name))
                    || content.contains(&format!("fn test_{}", fn_name));
                if is_pub_fn {
                    pub_items.push(signature.clone());
                }
                functions.push(FunctionInfo {
                    name: fn_name,
                    signature,
                    is_public: is_pub_fn,
                    has_test,
                    file: file_path.to_string(),
                });
            }
        }

        // ── Route detection ────────────────────────────────────────────────
        for method in &["get", "post", "put", "delete", "patch"] {
            let pattern = format!(".{}(\"", method);
            if let Some(pos) = t.find(&pattern) {
                let rest = &t[pos + pattern.len()..];
                if let Some(end) = rest.find('"') {
                    let path_str = &rest[..end];
                    routes.push(RouteInfo {
                        method: method.to_uppercase(),
                        path: path_str.to_string(),
                        file: file_path.to_string(),
                    });
                }
            }
        }
        // axum .route("path", ...)
        if let Some(pos) = t.find(".route(\"") {
            let rest = &t[pos + 8..];
            if let Some(end) = rest.find('"') {
                routes.push(RouteInfo {
                    method: "ROUTE".to_string(),
                    path: rest[..end].to_string(),
                    file: file_path.to_string(),
                });
            }
        }
    }

    (types, functions, routes, pub_items)
}

// ─── String Helpers ───────────────────────────────────────────────────────────

/// Extract derive names from a `#[derive(A, B, C)]` line.
fn extract_derives(line: &str) -> Vec<String> {
    let start = match line.find("derive(") {
        Some(i) => i + 7,
        None => return vec![],
    };
    let rest = &line[start..];
    let end = rest.find(')').unwrap_or(rest.len());
    rest[..end]
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Extract the leading identifier from a string.
fn ident_head(s: &str) -> String {
    s.chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect()
}

/// Extract function name and signature from a `fn ...` line.
fn extract_fn(line: &str) -> (String, String) {
    // Normalize async
    let sig = line.trim_end_matches('{').trim().to_string();

    // Find "fn " to locate the name
    let fn_idx = match sig.find("fn ") {
        Some(i) => i,
        None => return (String::new(), sig),
    };
    let after = &sig[fn_idx + 3..];
    let name = ident_head(after);
    (name, sig)
}

/// Smart truncation: keep first and last `max/2` chars.
pub fn smart_truncate(content: &str, max: usize) -> String {
    if content.len() <= max {
        return content.to_string();
    }
    let half = max / 2;
    let first = &content[..half];
    // Try to truncate at a newline boundary
    let first = match first.rfind('\n') {
        Some(i) => &content[..i],
        None => first,
    };
    let tail_start = content.len().saturating_sub(half);
    let last = &content[tail_start..];
    let last = match last.find('\n') {
        Some(i) => &last[i + 1..],
        None => last,
    };
    format!("{}\n// ... ({} chars truncated) ...\n{}", first, content.len() - first.len() - last.len(), last)
}

// ─── Tests (AT-AG13, AT-AG14) ─────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn make_temp_project(files: &[(&str, &str)]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "isls_ws_test_{:016x}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .subsec_nanos() as u64
        ));
        fs::create_dir_all(dir.join("src")).unwrap();
        for (path, content) in files {
            let full = dir.join(path);
            if let Some(parent) = full.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(&full, content).unwrap();
        }
        dir
    }

    fn basic_cargo_toml() -> &'static str {
        "[package]\nname = \"test-project\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nserde = \"1\"\ntokio = \"1\"\n"
    }

    // AT-AG13: Workspace analysis detects modules, types, functions
    #[test]
    fn at_ag13_workspace_analysis() {
        let dir = make_temp_project(&[
            ("Cargo.toml", basic_cargo_toml()),
            ("src/lib.rs", "pub struct Bookmark { pub id: i64, pub title: String }\npub fn list_bookmarks() -> Vec<Bookmark> { vec![] }\n"),
            ("src/database.rs", "use crate::Bookmark;\npub async fn fetch_all(pool: &str) -> Vec<Bookmark> { vec![] }\n"),
        ]);

        let ws = AgentWorkspace::analyze(&dir).expect("analyze");
        assert_eq!(ws.file_count, 2, "two .rs files");
        assert!(ws.loc >= 4, "at least 4 lines of code");
        assert!(ws.modules.len() >= 2, "at least 2 modules");
        assert!(!ws.types.is_empty(), "at least one type detected");
        assert!(!ws.functions.is_empty(), "at least one function detected");
        assert_eq!(ws.dependencies, vec!["serde", "tokio"], "dependencies parsed");
        assert_eq!(ws.crate_type, CrateType::Lib);

        let summary = ws.summary();
        assert!(summary.contains("Lib"), "summary contains crate type");
        assert!(summary.contains("module"), "summary mentions modules");
    }

    // AT-AG14: relevant_files finds correct modules for a task description
    #[test]
    fn at_ag14_relevant_file_detection() {
        let dir = make_temp_project(&[
            ("Cargo.toml", basic_cargo_toml()),
            ("src/search.rs", "pub fn search_bookmarks(query: &str) -> Vec<String> { vec![] }\n"),
            ("src/auth.rs", "pub fn authenticate(token: &str) -> bool { false }\n"),
            ("src/router.rs", "pub fn build_router() {}\n"),
            ("src/models.rs", "pub struct Bookmark { pub id: i64 }\n"),
            ("src/database.rs", "pub fn get_all() -> Vec<String> { vec![] }\n"),
        ]);
        let ws = AgentWorkspace::analyze(&dir).expect("analyze");

        // Task mentioning "search" should find search.rs first
        let relevant = ws.relevant_files("add search endpoint");
        assert!(!relevant.is_empty(), "must find at least one relevant file");
        let first_path = &relevant[0].path;
        assert!(
            first_path.contains("search") || first_path.contains("router"),
            "top match should be search or router, got: {}",
            first_path
        );

        // Task mentioning "auth" should find auth.rs
        let auth_relevant = ws.relevant_files("fix authentication bug");
        let has_auth = auth_relevant.iter().any(|m| m.path.contains("auth"));
        assert!(has_auth, "auth.rs should be in relevant files for auth task");
    }

    // AT-AG14b: smart_truncate preserves short content unchanged
    #[test]
    fn at_ag14b_smart_truncate_short() {
        let content = "fn foo() {}\n";
        assert_eq!(smart_truncate(content, 2000), content);
    }

    // AT-AG14c: smart_truncate trims long content
    #[test]
    fn at_ag14c_smart_truncate_long() {
        let content = "x".repeat(5000);
        let result = smart_truncate(&content, 100);
        assert!(result.len() < content.len(), "truncated");
        assert!(result.contains("truncated"), "truncation marker present");
    }
}
