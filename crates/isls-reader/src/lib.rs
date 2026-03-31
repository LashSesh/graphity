// Copyright (c) 2026 Sebastian Klemm
// SPDX-License-Identifier: MIT
//
//! Multi-language source code reader for ISLS — ported from Barbara codex-parse.
//!
//! Parses source files using regex-based analysis to extract structural
//! information: function definitions, struct/class declarations, imports,
//! SQL table names. Supports Rust, JavaScript, Python, SQL, HTML, CSS,
//! TOML, and Dockerfile.

use std::path::{Path, PathBuf};
use std::fs;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use sha2::{Digest, Sha256};
use regex::Regex;

// ─── Error ───────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum ReaderError {
    #[error("io error reading {path}: {source}")]
    Io { path: PathBuf, source: std::io::Error },
    #[error("unsupported language: {0}")]
    UnsupportedLanguage(String),
    #[error("regex error: {0}")]
    Regex(#[from] regex::Error),
    #[error("utf8 decode error: {0}")]
    Utf8(#[from] std::string::FromUtf8Error),
}

pub type Result<T> = std::result::Result<T, ReaderError>;

// ─── Language ────────────────────────────────────────────────────────────────

/// Source language detected from file extension or explicit specification.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Language {
    Rust,
    JavaScript,
    TypeScript,
    Python,
    Sql,
    Html,
    Css,
    Toml,
    Dockerfile,
    Unknown,
}

impl Language {
    /// Detect language from file extension.
    pub fn from_path(path: &Path) -> Self {
        match path.extension().and_then(|e| e.to_str()) {
            Some("rs") => Language::Rust,
            Some("js") | Some("mjs") => Language::JavaScript,
            Some("ts") | Some("tsx") => Language::TypeScript,
            Some("py") => Language::Python,
            Some("sql") => Language::Sql,
            Some("html") | Some("htm") => Language::Html,
            Some("css") => Language::Css,
            Some("toml") => Language::Toml,
            _ => {
                // Check filename for Dockerfile
                if path.file_name().and_then(|n| n.to_str())
                    .map(|n| n.starts_with("Dockerfile"))
                    .unwrap_or(false)
                {
                    Language::Dockerfile
                } else {
                    Language::Unknown
                }
            }
        }
    }

    /// Parse from explicit string name.
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "rust" | "rs" => Language::Rust,
            "javascript" | "js" => Language::JavaScript,
            "typescript" | "ts" => Language::TypeScript,
            "python" | "py" => Language::Python,
            "sql" => Language::Sql,
            "html" => Language::Html,
            "css" => Language::Css,
            "toml" => Language::Toml,
            "dockerfile" => Language::Dockerfile,
            _ => Language::Unknown,
        }
    }

    /// Returns the language name as a static string.
    pub fn as_str(&self) -> &'static str {
        match self {
            Language::Rust => "rust",
            Language::JavaScript => "javascript",
            Language::TypeScript => "typescript",
            Language::Python => "python",
            Language::Sql => "sql",
            Language::Html => "html",
            Language::Css => "css",
            Language::Toml => "toml",
            Language::Dockerfile => "dockerfile",
            Language::Unknown => "unknown",
        }
    }
}

// ─── Structural types ─────────────────────────────────────────────────────────

/// A function or method definition extracted from source.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FunctionDef {
    pub name: String,
    pub is_public: bool,
    pub is_async: bool,
    pub params: Vec<String>,
    pub return_type: Option<String>,
    pub line: usize,
}

/// A struct, class, or type definition extracted from source.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StructDef {
    pub name: String,
    pub is_public: bool,
    pub fields: Vec<String>,
    pub derives: Vec<String>,
    pub line: usize,
}

/// A SQL table definition.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SqlTable {
    pub name: String,
    pub columns: Vec<String>,
}

// ─── CodeObservation ─────────────────────────────────────────────────────────

/// Complete structural analysis of a single source file.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CodeObservation {
    /// Path of the source file (may be synthetic for `parse_string`).
    pub file_path: PathBuf,
    /// Detected or specified language.
    pub language: Language,
    /// All function/method definitions found.
    pub functions: Vec<FunctionDef>,
    /// All struct/class/type definitions found.
    pub structs: Vec<StructDef>,
    /// Import/use/require statements.
    pub imports: Vec<String>,
    /// SQL table names (from SQL files or inline SQL strings).
    pub sql_tables: Vec<SqlTable>,
    /// Total lines of code (non-empty, non-comment).
    pub loc: usize,
    /// SHA-256 of the raw source bytes.
    pub sha256: String,
}

// ─── WorkspaceAnalysis ───────────────────────────────────────────────────────

/// Aggregated analysis of an entire directory tree.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkspaceAnalysis {
    pub root: PathBuf,
    pub files: Vec<CodeObservation>,
    pub total_loc: usize,
    pub total_functions: usize,
    pub total_structs: usize,
    pub languages: Vec<String>,
}

// ─── SHA helper ──────────────────────────────────────────────────────────────

fn sha256_hex(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    h.finalize().iter().map(|b| format!("{:02x}", b)).collect()
}

// ─── Rust parser ─────────────────────────────────────────────────────────────

fn parse_rust(source: &str) -> (Vec<FunctionDef>, Vec<StructDef>, Vec<String>) {
    let mut functions = Vec::new();
    let mut structs = Vec::new();
    let mut imports = Vec::new();

    // use statements
    let use_re = Regex::new(r"^use\s+([\w::{}, *]+);").unwrap();
    // pub fn / fn definitions (single line signature capture)
    let fn_re = Regex::new(
        r"(?m)^[ \t]*(pub(?:\(crate\))?\s+)?(async\s+)?fn\s+(\w+)\s*\(([^)]*)\)(?:\s*->\s*([^{;]+?))?[\s\{;]"
    ).unwrap();
    // struct definitions
    let struct_re = Regex::new(r"(?m)^[ \t]*(pub(?:\(crate\))?\s+)?struct\s+(\w+)").unwrap();
    // derive macros
    let derive_re = Regex::new(r"#\[derive\(([^)]+)\)\]").unwrap();

    for (line_idx, line) in source.lines().enumerate() {
        let trimmed = line.trim();
        if let Some(cap) = use_re.captures(trimmed) {
            imports.push(cap[1].to_string());
        }
        if let Some(cap) = struct_re.captures(line) {
            let is_public = cap.get(1).is_some();
            let name = cap[2].to_string();
            // Collect derives from the previous few lines
            let start = line_idx.saturating_sub(3);
            let context: String = source.lines()
                .skip(start)
                .take(line_idx - start + 1)
                .collect::<Vec<_>>()
                .join("\n");
            let derives: Vec<String> = derive_re.captures_iter(&context)
                .flat_map(|c| c[1].split(',').map(|s| s.trim().to_string()).collect::<Vec<_>>())
                .collect();
            structs.push(StructDef { name, is_public, fields: vec![], derives, line: line_idx + 1 });
        }
    }

    for cap in fn_re.captures_iter(source) {
        let is_public = cap.get(1).is_some();
        let is_async = cap.get(2).is_some();
        let name = cap[3].to_string();
        let params_raw = cap[4].trim().to_string();
        let return_type = cap.get(5).map(|m| m.as_str().trim().to_string());
        let params: Vec<String> = if params_raw.is_empty() {
            vec![]
        } else {
            params_raw.split(',').map(|s| s.trim().to_string()).collect()
        };
        // Approximate line number
        let line = source[..cap.get(0).unwrap().start()]
            .chars()
            .filter(|&c| c == '\n')
            .count() + 1;
        functions.push(FunctionDef { name, is_public, is_async, params, return_type, line });
    }

    (functions, structs, imports)
}

// ─── JavaScript / TypeScript parser ──────────────────────────────────────────

fn parse_javascript(source: &str) -> (Vec<FunctionDef>, Vec<StructDef>, Vec<String>) {
    let mut functions = Vec::new();
    let mut structs = Vec::new();
    let mut imports = Vec::new();

    let import_re = Regex::new(r#"import\s+.*?\s+from\s+['"]([^'"]+)['"]"#).unwrap();
    let require_re = Regex::new(r#"require\s*\(\s*['"]([^'"]+)['"]\s*\)"#).unwrap();
    let fn_re = Regex::new(
        r"(?m)(async\s+)?function\s+(\w+)\s*\(([^)]*)\)|(?:const|let|var)\s+(\w+)\s*=\s*(async\s+)?\([^)]*\)\s*=>"
    ).unwrap();
    let class_re = Regex::new(r"(?m)class\s+(\w+)").unwrap();

    for cap in import_re.captures_iter(source) {
        imports.push(cap[1].to_string());
    }
    for cap in require_re.captures_iter(source) {
        imports.push(cap[1].to_string());
    }

    for cap in fn_re.captures_iter(source) {
        let is_async = cap.get(1).is_some() || cap.get(5).is_some();
        let name = cap.get(2).or_else(|| cap.get(4))
            .map(|m| m.as_str().to_string())
            .unwrap_or_default();
        if name.is_empty() { continue; }
        let params_raw = cap.get(3).map(|m| m.as_str()).unwrap_or("").trim().to_string();
        let params: Vec<String> = if params_raw.is_empty() {
            vec![]
        } else {
            params_raw.split(',').map(|s| s.trim().to_string()).collect()
        };
        let line = source[..cap.get(0).unwrap().start()]
            .chars().filter(|&c| c == '\n').count() + 1;
        functions.push(FunctionDef { name, is_public: true, is_async, params, return_type: None, line });
    }

    for cap in class_re.captures_iter(source) {
        structs.push(StructDef {
            name: cap[1].to_string(),
            is_public: true,
            fields: vec![],
            derives: vec![],
            line: source[..cap.get(0).unwrap().start()].chars().filter(|&c| c == '\n').count() + 1,
        });
    }

    (functions, structs, imports)
}

// ─── SQL parser ──────────────────────────────────────────────────────────────

fn parse_sql(source: &str) -> Vec<SqlTable> {
    let mut tables = Vec::new();
    let create_re = Regex::new(
        r#"(?i)CREATE\s+TABLE\s+(?:IF\s+NOT\s+EXISTS\s+)?"?(\w+)"?\s*\(([^;]+)"#
    ).unwrap();
    let col_re = Regex::new(r#"(?m)^\s+"?(\w+)"?\s+\w"#).unwrap();

    for cap in create_re.captures_iter(source) {
        let name = cap[1].to_string();
        let body = cap[2].to_string();
        let columns: Vec<String> = col_re.captures_iter(&body)
            .map(|c| c[1].to_string())
            .filter(|c| !["PRIMARY", "UNIQUE", "INDEX", "KEY", "CONSTRAINT", "FOREIGN"].contains(&c.to_uppercase().as_str()))
            .collect();
        tables.push(SqlTable { name, columns });
    }
    tables
}

// ─── Python parser ───────────────────────────────────────────────────────────

fn parse_python(source: &str) -> (Vec<FunctionDef>, Vec<StructDef>, Vec<String>) {
    let mut functions = Vec::new();
    let mut structs = Vec::new();
    let mut imports = Vec::new();

    let import_re = Regex::new(r"^(?:import|from)\s+([\w.]+)").unwrap();
    let fn_re = Regex::new(r"(?m)^(\s*)(?:async\s+)?def\s+(\w+)\s*\(([^)]*)\)").unwrap();
    let class_re = Regex::new(r"(?m)^class\s+(\w+)").unwrap();

    for line in source.lines() {
        if let Some(cap) = import_re.captures(line.trim()) {
            imports.push(cap[1].to_string());
        }
    }
    for cap in fn_re.captures_iter(source) {
        let indent = cap[1].len();
        let is_public = !cap[2].starts_with('_');
        let name = cap[2].to_string();
        let params_raw = cap[3].trim().to_string();
        let params: Vec<String> = params_raw.split(',').map(|s| s.trim().to_string()).collect();
        let line = source[..cap.get(0).unwrap().start()].chars().filter(|&c| c == '\n').count() + 1;
        let _ = indent;
        functions.push(FunctionDef { name, is_public, is_async: false, params, return_type: None, line });
    }
    for cap in class_re.captures_iter(source) {
        let line = source[..cap.get(0).unwrap().start()].chars().filter(|&c| c == '\n').count() + 1;
        structs.push(StructDef { name: cap[1].to_string(), is_public: true, fields: vec![], derives: vec![], line });
    }

    (functions, structs, imports)
}

// ─── LOC counter ─────────────────────────────────────────────────────────────

fn count_loc(source: &str) -> usize {
    source.lines()
        .filter(|l| {
            let t = l.trim();
            !t.is_empty() && !t.starts_with("//") && !t.starts_with('#') && !t.starts_with("--")
        })
        .count()
}

// ─── Public API ──────────────────────────────────────────────────────────────

/// Parse a single source file and return its structural observation.
pub fn parse_file(path: &Path) -> Result<CodeObservation> {
    let bytes = fs::read(path).map_err(|e| ReaderError::Io { path: path.to_path_buf(), source: e })?;
    let source = String::from_utf8(bytes.clone())
        .map_err(ReaderError::Utf8)?;
    let language = Language::from_path(path);
    let sha256 = sha256_hex(&bytes);
    Ok(build_observation(path.to_path_buf(), source, language, sha256))
}

/// Parse source code given as a string with an explicit language hint.
pub fn parse_string(source: &str, language: Language) -> Result<CodeObservation> {
    let sha256 = sha256_hex(source.as_bytes());
    Ok(build_observation(PathBuf::from("<memory>"), source.to_string(), language, sha256))
}

/// Recursively parse all source files in a directory.
pub fn parse_directory(dir: &Path) -> Result<WorkspaceAnalysis> {
    let mut files = Vec::new();
    collect_files(dir, &mut files)?;

    let total_loc = files.iter().map(|f| f.loc).sum();
    let total_functions = files.iter().map(|f| f.functions.len()).sum();
    let total_structs = files.iter().map(|f| f.structs.len()).sum();
    let mut langs: Vec<String> = files.iter().map(|f| f.language.as_str().to_string()).collect();
    langs.sort();
    langs.dedup();

    Ok(WorkspaceAnalysis {
        root: dir.to_path_buf(),
        files,
        total_loc,
        total_functions,
        total_structs,
        languages: langs,
    })
}

fn collect_files(dir: &Path, out: &mut Vec<CodeObservation>) -> Result<()> {
    let entries = fs::read_dir(dir).map_err(|e| ReaderError::Io { path: dir.to_path_buf(), source: e })?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // Skip hidden dirs and common non-source dirs
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !name.starts_with('.') && name != "target" && name != "node_modules" {
                collect_files(&path, out)?;
            }
        } else if path.is_file() {
            let lang = Language::from_path(&path);
            if !matches!(lang, Language::Unknown) {
                if let Ok(obs) = parse_file(&path) {
                    out.push(obs);
                }
            }
        }
    }
    Ok(())
}

fn build_observation(file_path: PathBuf, source: String, language: Language, sha256: String) -> CodeObservation {
    let loc = count_loc(&source);
    let (functions, structs, imports, sql_tables) = match &language {
        Language::Rust => {
            let (f, s, i) = parse_rust(&source);
            (f, s, i, vec![])
        }
        Language::JavaScript | Language::TypeScript => {
            let (f, s, i) = parse_javascript(&source);
            (f, s, i, vec![])
        }
        Language::Python => {
            let (f, s, i) = parse_python(&source);
            (f, s, i, vec![])
        }
        Language::Sql => {
            let tables = parse_sql(&source);
            (vec![], vec![], vec![], tables)
        }
        _ => (vec![], vec![], vec![], vec![]),
    };
    CodeObservation { file_path, language, functions, structs, imports, sql_tables, loc, sha256 }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_rust_functions() {
        let src = r#"
use std::io;

pub struct Foo { pub x: i32 }

pub fn greet(name: &str) -> String {
    format!("Hello, {}", name)
}

async fn fetch(url: &str) -> Result<()> {
    Ok(())
}
"#;
        let obs = parse_string(src, Language::Rust).unwrap();
        assert!(obs.functions.iter().any(|f| f.name == "greet" && f.is_public));
        assert!(obs.functions.iter().any(|f| f.name == "fetch" && f.is_async));
        assert!(obs.structs.iter().any(|s| s.name == "Foo"));
        assert!(obs.imports.iter().any(|i| i.contains("std::io")));
    }

    #[test]
    fn parse_sql_tables() {
        let src = r#"
CREATE TABLE IF NOT EXISTS products (
    id BIGSERIAL PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    sku VARCHAR(64) UNIQUE,
    quantity INTEGER DEFAULT 0
);
"#;
        let obs = parse_string(src, Language::Sql).unwrap();
        assert!(obs.sql_tables.iter().any(|t| t.name == "products"));
        let products = obs.sql_tables.iter().find(|t| t.name == "products").unwrap();
        assert!(products.columns.iter().any(|c| c == "name"));
    }

    #[test]
    fn parse_js_functions() {
        let src = r#"
import { fetchData } from './api';

async function loadProducts() {
    const data = await fetchData('/api/products');
    return data;
}

const createProduct = async (body) => {
    return fetch('/api/products', { method: 'POST', body: JSON.stringify(body) });
};
"#;
        let obs = parse_string(src, Language::JavaScript).unwrap();
        assert!(obs.functions.iter().any(|f| f.name == "loadProducts"));
    }

    #[test]
    fn language_detection() {
        assert_eq!(Language::from_path(Path::new("main.rs")), Language::Rust);
        assert_eq!(Language::from_path(Path::new("app.js")), Language::JavaScript);
        assert_eq!(Language::from_path(Path::new("schema.sql")), Language::Sql);
        assert_eq!(Language::from_path(Path::new("Dockerfile")), Language::Dockerfile);
    }
}
