// ── C27 §4: Workspace Intelligence ──────────────────────────────────
//
// Scans an existing Rust project and builds a model of its structure.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

use crate::Result;

// ── Public Types ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum CrateType {
    Bin,
    Lib,
    Workspace,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TypeKind {
    Struct,
    Enum,
    TypeAlias,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleInfo {
    pub name: String,
    pub path: String,
    pub public_items: Vec<String>,
    pub imports: Vec<String>,
    pub loc: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeInfo {
    pub name: String,
    pub kind: TypeKind,
    pub fields: Vec<String>,
    pub derives: Vec<String>,
    pub module: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraitInfo {
    pub name: String,
    pub methods: Vec<String>,
    pub implementors: Vec<String>,
    pub module: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionInfo {
    pub name: String,
    pub signature: String,
    pub is_public: bool,
    pub has_test: bool,
    pub module: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DepInfo {
    pub name: String,
    pub version: String,
    pub features: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceModel {
    pub project_name: String,
    pub crate_type: CrateType,
    pub modules: Vec<ModuleInfo>,
    pub types: Vec<TypeInfo>,
    pub traits: Vec<TraitInfo>,
    pub functions: Vec<FunctionInfo>,
    pub dependencies: Vec<DepInfo>,
    pub test_count: usize,
    pub loc: usize,
    pub file_tree: Vec<String>,
}

/// Context passed to the Oracle when synthesising code for an existing
/// project, so generated code uses existing types / patterns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceContext {
    pub summary: String,
    pub relevant_types: Vec<TypeInfo>,
    pub relevant_traits: Vec<TraitInfo>,
    pub relevant_imports: Vec<String>,
    pub existing_patterns: Vec<String>,
    pub file_being_modified: Option<String>,
}

// ── Analyzer ────────────────────────────────────────────────────────

pub struct WorkspaceAnalyzer;

impl WorkspaceAnalyzer {
    /// Analyse the Rust project rooted at `dir`.
    pub fn analyze(dir: &Path) -> Result<WorkspaceModel> {
        let project_name = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "unknown".into());

        let crate_type = Self::detect_crate_type(dir);
        let file_tree = Self::collect_file_tree(dir, dir);

        let mut modules = Vec::new();
        let mut types = Vec::new();
        let mut traits = Vec::new();
        let mut functions = Vec::new();
        let mut test_count: usize = 0;
        let mut total_loc: usize = 0;

        // Scan all .rs files
        for rel in &file_tree {
            if !rel.ends_with(".rs") {
                continue;
            }
            let full = dir.join(rel);
            let content = match fs::read_to_string(&full) {
                Ok(c) => c,
                Err(_) => continue,
            };

            let lines: Vec<&str> = content.lines().collect();
            let loc = lines.len();
            total_loc += loc;

            let mod_name = rel
                .trim_end_matches(".rs")
                .replace('/', "::")
                .replace("src::", "");

            let mut public_items = Vec::new();
            let mut imports = Vec::new();

            for line in &lines {
                let trimmed = line.trim();

                // Imports
                if trimmed.starts_with("use ") {
                    imports.push(trimmed.to_string());
                }

                // Public items
                if trimmed.starts_with("pub fn ")
                    || trimmed.starts_with("pub async fn ")
                    || trimmed.starts_with("pub struct ")
                    || trimmed.starts_with("pub enum ")
                    || trimmed.starts_with("pub trait ")
                    || trimmed.starts_with("pub type ")
                    || trimmed.starts_with("pub const ")
                {
                    public_items.push(trimmed.to_string());
                }

                // Types
                if let Some(rest) = trimmed.strip_prefix("pub struct ") {
                    let name = rest.split([' ', '{', '(', '<', ';']).next().unwrap_or("");
                    types.push(TypeInfo {
                        name: name.to_string(),
                        kind: TypeKind::Struct,
                        fields: Vec::new(),
                        derives: Self::extract_derives(&lines, line),
                        module: mod_name.clone(),
                    });
                }
                if let Some(rest) = trimmed.strip_prefix("pub enum ") {
                    let name = rest.split([' ', '{', '<']).next().unwrap_or("");
                    types.push(TypeInfo {
                        name: name.to_string(),
                        kind: TypeKind::Enum,
                        fields: Vec::new(),
                        derives: Self::extract_derives(&lines, line),
                        module: mod_name.clone(),
                    });
                }

                // Traits
                if let Some(rest) = trimmed.strip_prefix("pub trait ") {
                    let name = rest.split([' ', '{', '<', ':']).next().unwrap_or("");
                    traits.push(TraitInfo {
                        name: name.to_string(),
                        methods: Vec::new(),
                        implementors: Vec::new(),
                        module: mod_name.clone(),
                    });
                }

                // Functions
                if trimmed.starts_with("pub fn ") || trimmed.starts_with("pub async fn ") {
                    let sig = trimmed.to_string();
                    let name = Self::extract_fn_name(trimmed);
                    functions.push(FunctionInfo {
                        name,
                        signature: sig,
                        is_public: true,
                        has_test: false,
                        module: mod_name.clone(),
                    });
                }

                // Test count
                if trimmed == "#[test]"
                    || trimmed.starts_with("#[test]")
                    || trimmed.starts_with("#[tokio::test")
                {
                    test_count += 1;
                }
            }

            modules.push(ModuleInfo {
                name: mod_name,
                path: rel.clone(),
                public_items,
                imports,
                loc,
            });
        }

        // Mark functions that have corresponding tests
        let test_names: Vec<String> = functions
            .iter()
            .filter(|f| f.signature.contains("#[test]"))
            .map(|f| f.name.clone())
            .collect();
        for f in &mut functions {
            if test_names.iter().any(|t| t.contains(&f.name)) {
                f.has_test = true;
            }
        }

        let dependencies = Self::parse_dependencies(dir);

        Ok(WorkspaceModel {
            project_name,
            crate_type,
            modules,
            types,
            traits,
            functions,
            dependencies,
            test_count,
            loc: total_loc,
            file_tree,
        })
    }

    /// Build a WorkspaceContext scoped to entities relevant to `hint`.
    pub fn build_context(model: &WorkspaceModel, hint: &str) -> WorkspaceContext {
        let hint_lower = hint.to_lowercase();

        let relevant_types: Vec<TypeInfo> = model
            .types
            .iter()
            .filter(|t| hint_lower.contains(&t.name.to_lowercase()))
            .cloned()
            .collect();

        let relevant_traits: Vec<TraitInfo> = model
            .traits
            .iter()
            .filter(|t| hint_lower.contains(&t.name.to_lowercase()))
            .cloned()
            .collect();

        let relevant_imports: Vec<String> = model
            .modules
            .iter()
            .flat_map(|m| m.imports.iter())
            .filter(|i| {
                relevant_types.iter().any(|t| i.contains(&t.name))
                    || relevant_traits.iter().any(|t| i.contains(&t.name))
            })
            .cloned()
            .collect();

        let existing_patterns: Vec<String> = model
            .modules
            .iter()
            .flat_map(|m| m.public_items.iter())
            .take(20)
            .cloned()
            .collect();

        let summary = format!(
            "{} ({:?}), {} modules, {} types, {} traits, {} fns, {} tests, {} LOC",
            model.project_name,
            model.crate_type,
            model.modules.len(),
            model.types.len(),
            model.traits.len(),
            model.functions.len(),
            model.test_count,
            model.loc,
        );

        WorkspaceContext {
            summary,
            relevant_types,
            relevant_traits,
            relevant_imports,
            existing_patterns,
            file_being_modified: None,
        }
    }

    // ── helpers ──────────────────────────────────────────────────────

    fn detect_crate_type(dir: &Path) -> CrateType {
        let cargo = dir.join("Cargo.toml");
        if let Ok(content) = fs::read_to_string(&cargo) {
            if content.contains("[workspace]") {
                return CrateType::Workspace;
            }
        }
        if dir.join("src/main.rs").exists() {
            CrateType::Bin
        } else {
            CrateType::Lib
        }
    }

    fn collect_file_tree(base: &Path, current: &Path) -> Vec<String> {
        let mut result = Vec::new();
        let entries = match fs::read_dir(current) {
            Ok(e) => e,
            Err(_) => return result,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if let Ok(rel) = path.strip_prefix(base) {
                let rel_str = rel.to_string_lossy().into_owned();
                // Skip hidden dirs, target/
                if rel_str.starts_with('.') || rel_str.starts_with("target") {
                    continue;
                }
                if path.is_dir() {
                    result.extend(Self::collect_file_tree(base, &path));
                } else {
                    result.push(rel_str);
                }
            }
        }
        result.sort();
        result
    }

    fn extract_fn_name(line: &str) -> String {
        let after_fn = if let Some(i) = line.find("fn ") {
            &line[i + 3..]
        } else {
            line
        };
        after_fn
            .split(['(', '<', ' '])
            .next()
            .unwrap_or("")
            .to_string()
    }

    fn extract_derives(lines: &[&str], _current: &str) -> Vec<String> {
        // Simple: look for #[derive(...)] in the preceding context
        // This is a best-effort scan; we search all lines for derives
        // matching nearby context.
        let mut derives = Vec::new();
        for l in lines {
            if let Some(inner) = l.trim().strip_prefix("#[derive(") {
                if let Some(content) = inner.strip_suffix(")]") {
                    for d in content.split(',') {
                        derives.push(d.trim().to_string());
                    }
                }
            }
        }
        derives.dedup();
        derives
    }

    fn parse_dependencies(dir: &Path) -> Vec<DepInfo> {
        let cargo = dir.join("Cargo.toml");
        let content = match fs::read_to_string(&cargo) {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };
        let mut deps = Vec::new();
        let mut in_deps = false;
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed == "[dependencies]" || trimmed == "[dev-dependencies]" {
                in_deps = true;
                continue;
            }
            if trimmed.starts_with('[') {
                in_deps = false;
                continue;
            }
            if in_deps {
                if let Some((name, rest)) = trimmed.split_once('=') {
                    let name = name.trim().trim_matches('"').to_string();
                    let version = rest
                        .trim()
                        .trim_matches('"')
                        .trim_start_matches("{ version = \"")
                        .split('"')
                        .next()
                        .unwrap_or("*")
                        .to_string();
                    deps.push(DepInfo {
                        name,
                        version,
                        features: Vec::new(),
                    });
                }
            }
        }
        deps
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn detect_lib_crate() {
        let dir = std::env::temp_dir().join("ws_test_lib");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(dir.join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
        fs::write(dir.join("src/lib.rs"), "pub fn hello() {}").unwrap();
        assert_eq!(WorkspaceAnalyzer::detect_crate_type(&dir), CrateType::Lib);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn detect_bin_crate() {
        let dir = std::env::temp_dir().join("ws_test_bin");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(dir.join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
        fs::write(dir.join("src/main.rs"), "fn main() {}").unwrap();
        assert_eq!(WorkspaceAnalyzer::detect_crate_type(&dir), CrateType::Bin);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn analyze_simple_project() {
        let dir = std::env::temp_dir().join("ws_test_analyze");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(dir.join("Cargo.toml"), "[package]\nname = \"demo\"\n[dependencies]\nserde = \"1\"\n").unwrap();
        fs::write(
            dir.join("src/lib.rs"),
            "pub struct Foo;\npub fn bar() -> u32 { 1 }\n#[test]\nfn test_bar() { assert_eq!(bar(), 1); }\n",
        ).unwrap();
        let model = WorkspaceAnalyzer::analyze(&dir).unwrap();
        assert_eq!(model.project_name, "ws_test_analyze");
        assert_eq!(model.crate_type, CrateType::Lib);
        assert!(!model.types.is_empty());
        assert!(!model.functions.is_empty());
        assert_eq!(model.test_count, 1);
        assert!(!model.dependencies.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn build_context_filters_types() {
        let model = WorkspaceModel {
            project_name: "test".into(),
            crate_type: CrateType::Lib,
            modules: Vec::new(),
            types: vec![
                TypeInfo {
                    name: "User".into(),
                    kind: TypeKind::Struct,
                    fields: vec!["id".into()],
                    derives: vec!["Debug".into()],
                    module: "models".into(),
                },
                TypeInfo {
                    name: "Config".into(),
                    kind: TypeKind::Struct,
                    fields: Vec::new(),
                    derives: Vec::new(),
                    module: "settings".into(),
                },
            ],
            traits: Vec::new(),
            functions: Vec::new(),
            dependencies: Vec::new(),
            test_count: 0,
            loc: 100,
            file_tree: Vec::new(),
        };
        let ctx = WorkspaceAnalyzer::build_context(&model, "User authentication");
        assert_eq!(ctx.relevant_types.len(), 1);
        assert_eq!(ctx.relevant_types[0].name, "User");
    }
}
