// ── C27 §3: Project Scaffolding ─────────────────────────────────────
//
// Generates complete, runnable Cargo project structures from
// forge artifacts.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use isls_compose::AtomArtifact;
use isls_pmhd::DecisionSpec;
use isls_templates::ArchitectureTemplate;

use crate::Result;

// ── Generated File Descriptor ───────────────────────────────────────

/// Source of a generated file's content.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub enum SynthesisSource {
    Oracle,
    Memory,
    Static,
    Derive,
}

/// A single file produced by the Foundry.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GeneratedFile {
    pub path: String,
    pub content: String,
    pub source: SynthesisSource,
    pub loc: usize,
    pub test_count: usize,
}

impl GeneratedFile {
    pub fn new(path: impl Into<String>, content: impl Into<String>, source: SynthesisSource) -> Self {
        let content = content.into();
        let loc = content.lines().count();
        let test_count = content.matches("#[test]").count()
            + content.matches("#[tokio::test").count();
        Self {
            path: path.into(),
            content,
            source,
            loc,
            test_count,
        }
    }
}

// ── Cargo.toml Builder ──────────────────────────────────────────────

pub struct CargoTomlBuilder;

impl CargoTomlBuilder {
    /// Build a `Cargo.toml` string from project metadata.
    pub fn build(
        name: &str,
        template: Option<&ArchitectureTemplate>,
        atoms: &[AtomArtifact],
        extra_deps: &BTreeMap<String, String>,
    ) -> String {
        let mut out = String::new();
        out.push_str("[package]\n");
        out.push_str(&format!("name = \"{name}\"\n"));
        out.push_str("version = \"0.1.0\"\n");
        out.push_str("edition = \"2021\"\n\n");

        // Collect dependencies
        let mut deps: BTreeMap<String, String> = BTreeMap::new();

        // From template
        if let Some(tmpl) = template {
            for cap in &tmpl.required_capabilities {
                match cap.as_str() {
                    "axum" => {
                        deps.insert("axum".into(), "\"0.7\"".into());
                        deps.insert(
                            "tokio".into(),
                            "{ version = \"1\", features = [\"full\"] }".into(),
                        );
                    }
                    "serde" => {
                        deps.insert(
                            "serde".into(),
                            "{ version = \"1\", features = [\"derive\"] }".into(),
                        );
                        deps.insert("serde_json".into(), "\"1\"".into());
                    }
                    "sqlx" | "database" => {
                        deps.insert(
                            "sqlx".into(),
                            "{ version = \"0.7\", features = [\"runtime-tokio\", \"sqlite\"] }"
                                .into(),
                        );
                    }
                    "clap" => {
                        deps.insert(
                            "clap".into(),
                            "{ version = \"4\", features = [\"derive\"] }".into(),
                        );
                    }
                    other => {
                        deps.insert(other.into(), "\"*\"".into());
                    }
                }
            }
        }

        // Infer from atom content
        for atom in atoms {
            let content = &atom.synthesis.content;
            if content.contains("use serde") && !deps.contains_key("serde") {
                deps.insert(
                    "serde".into(),
                    "{ version = \"1\", features = [\"derive\"] }".into(),
                );
            }
            if content.contains("serde_json") && !deps.contains_key("serde_json") {
                deps.insert("serde_json".into(), "\"1\"".into());
            }
            if content.contains("use tokio") && !deps.contains_key("tokio") {
                deps.insert(
                    "tokio".into(),
                    "{ version = \"1\", features = [\"full\"] }".into(),
                );
            }
        }

        // Extra / user-supplied deps
        for (k, v) in extra_deps {
            deps.insert(k.clone(), v.clone());
        }

        if !deps.is_empty() {
            out.push_str("[dependencies]\n");
            for (k, v) in &deps {
                out.push_str(&format!("{k} = {v}\n"));
            }
        }

        out
    }
}

// ── Project Scaffold ────────────────────────────────────────────────

pub struct ProjectScaffold;

impl ProjectScaffold {
    /// Write a complete project scaffold to `dir`.
    ///
    /// Returns the list of files written.
    #[allow(clippy::too_many_arguments)]
    pub fn write_project(
        dir: &Path,
        name: &str,
        spec: &DecisionSpec,
        template: Option<&ArchitectureTemplate>,
        atoms: &[AtomArtifact],
        extra_deps: &BTreeMap<String, String>,
        generate_readme: bool,
        generate_gitignore: bool,
    ) -> Result<Vec<GeneratedFile>> {
        fs::create_dir_all(dir.join("src"))?;
        fs::create_dir_all(dir.join("tests"))?;

        let mut files = Vec::new();

        // Cargo.toml
        let cargo = CargoTomlBuilder::build(name, template, atoms, extra_deps);
        let cargo_file = GeneratedFile::new("Cargo.toml", &cargo, SynthesisSource::Static);
        fs::write(dir.join("Cargo.toml"), &cargo)?;
        files.push(cargo_file);

        // .gitignore
        if generate_gitignore {
            let gitignore = "/target\n*.swp\n*.swo\n.env\n";
            fs::write(dir.join(".gitignore"), gitignore)?;
            files.push(GeneratedFile::new(".gitignore", gitignore, SynthesisSource::Static));
        }

        // README.md
        if generate_readme {
            let readme = Self::build_readme(name, spec, template, atoms);
            fs::write(dir.join("README.md"), &readme)?;
            files.push(GeneratedFile::new("README.md", &readme, SynthesisSource::Static));
        }

        // Atom files → src/<name>.rs
        let mut mod_names = Vec::new();
        for atom in atoms {
            let file_name = sanitize_module_name(&atom.file_path);
            let src_path = format!("src/{file_name}");
            let full_path = dir.join(&src_path);

            // Ensure parent exists (for nested modules)
            if let Some(parent) = full_path.parent() {
                fs::create_dir_all(parent)?;
            }

            fs::write(&full_path, &atom.synthesis.content)?;

            let source = match &atom.synthesis.content {
                c if c.contains("// source: oracle") => SynthesisSource::Oracle,
                c if c.contains("// source: memory") => SynthesisSource::Memory,
                _ => SynthesisSource::Oracle,
            };

            files.push(GeneratedFile::new(&src_path, &atom.synthesis.content, source));
            mod_names.push(file_name.trim_end_matches(".rs").to_string());
        }

        // lib.rs or main.rs (entry point with mod declarations)
        let has_main = template
            .map(|t| {
                matches!(
                    t.archetype,
                    isls_templates::Archetype::RestApi
                        | isls_templates::Archetype::CliTool
                        | isls_templates::Archetype::Microservice
                        | isls_templates::Archetype::FullStackApp
                )
            })
            .unwrap_or(false);

        let entry_name = if has_main { "src/main.rs" } else { "src/lib.rs" };

        // Only write entry if it wasn't already produced by an atom
        if !files.iter().any(|f| f.path == entry_name) {
            let mut entry = String::new();
            for m in &mod_names {
                if *m != "main" && *m != "lib" {
                    entry.push_str(&format!("pub mod {m};\n"));
                }
            }
            if has_main {
                entry.push_str("\nfn main() {\n    println!(\"Hello from ISLS Foundry!\");\n}\n");
            }
            fs::write(dir.join(entry_name), &entry)?;
            files.push(GeneratedFile::new(entry_name, &entry, SynthesisSource::Static));
        }

        // Stub integration test
        let test_content = "#[test]\nfn project_compiles() {\n    // Smoke test: the project compiles.\n    assert!(true);\n}\n".to_string();
        fs::write(dir.join("tests/integration.rs"), &test_content)?;
        files.push(GeneratedFile::new("tests/integration.rs", &test_content, SynthesisSource::Static));

        Ok(files)
    }

    /// Write a single file into an existing project.
    pub fn write_file(dir: &Path, rel_path: &str, content: &str) -> Result<GeneratedFile> {
        let full = dir.join(rel_path);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&full, content)?;
        Ok(GeneratedFile::new(rel_path, content, SynthesisSource::Oracle))
    }

    fn build_readme(
        name: &str,
        spec: &DecisionSpec,
        template: Option<&ArchitectureTemplate>,
        atoms: &[AtomArtifact],
    ) -> String {
        let mut readme = format!("# {name}\n\n");
        readme.push_str(&format!("{}\n\n", spec.intent));
        readme.push_str("## Getting Started\n\n```\ncargo build\ncargo test\n```\n\n");

        if let Some(tmpl) = template {
            readme.push_str(&format!(
                "## Architecture\n\nTemplate: {} ({})\n\n",
                tmpl.name,
                tmpl.archetype.as_str(),
            ));
        }

        readme.push_str("## Modules\n\n");
        for atom in atoms {
            readme.push_str(&format!("- `{}`\n", atom.file_path));
        }

        readme.push_str(&format!(
            "\n## Generated by ISLS Foundry\n\nCrystal ID: {:02x?}\n",
            &spec.id[..4],
        ));
        readme
    }
}

fn sanitize_module_name(path: &str) -> String {
    let name = path
        .rsplit('/')
        .next()
        .unwrap_or(path)
        .replace('-', "_");
    if name.ends_with(".rs") {
        name
    } else {
        format!("{name}.rs")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cargo_toml_basic() {
        let toml = CargoTomlBuilder::build("hello", None, &[], &BTreeMap::new());
        assert!(toml.contains("name = \"hello\""));
        assert!(toml.contains("edition = \"2021\""));
    }

    #[test]
    fn cargo_toml_extra_deps() {
        let mut deps = BTreeMap::new();
        deps.insert("rand".into(), "\"0.8\"".into());
        let toml = CargoTomlBuilder::build("test", None, &[], &deps);
        assert!(toml.contains("rand = \"0.8\""));
    }

    #[test]
    fn sanitize_names() {
        assert_eq!(sanitize_module_name("my-module"), "my_module.rs");
        assert_eq!(sanitize_module_name("src/foo.rs"), "foo.rs");
        assert_eq!(sanitize_module_name("bar.rs"), "bar.rs");
    }

    #[test]
    fn generated_file_counts() {
        let f = GeneratedFile::new(
            "src/lib.rs",
            "fn a() {}\n#[test]\nfn t() {}\n#[test]\nfn t2() {}\n",
            SynthesisSource::Oracle,
        );
        assert_eq!(f.test_count, 2);
        assert_eq!(f.loc, 5);
    }
}
