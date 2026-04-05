// Copyright (c) 2026 Sebastian Klemm
// SPDX-License-Identifier: MIT
//
//! Regex-based TypeScript parser for ISLS I4.
//!
//! Extracts function declarations, arrow functions, interfaces, classes, type
//! aliases, and import statements from `.ts` / `.tsx` files.

use regex::Regex;
use crate::{FunctionDef, StructDef};
use crate::normalize::normalize_name;

// ─── Node built-ins (stdlib for TypeScript) ──────────────────────────────────

const TS_STDLIB: &[&str] = &[
    "fs", "path", "http", "https", "url", "crypto",
    "os", "child_process", "stream", "buffer",
    "events", "util", "assert", "net", "tls",
    "querystring", "readline", "zlib", "dns",
    "dgram", "cluster", "worker_threads", "perf_hooks",
    "v8", "vm", "module", "process",
];

/// Returns `true` when the import source is from an external npm package or
/// a path that is not relative (`./`, `../`) and not a Node built-in.
pub fn is_external_ts(path: &str) -> bool {
    if path.starts_with('.') || path.starts_with('/') {
        return false;
    }
    // Node built-ins (may be prefixed with "node:")
    let base = path.trim_start_matches("node:");
    !TS_STDLIB.contains(&base)
}

// ─── Parser ───────────────────────────────────────────────────────────────────

/// Parse TypeScript source and return (functions, types, imports).
///
/// Import strings: relative paths kept as-is; external packages prefixed with `ext:`.
pub fn parse_typescript(source: &str) -> (Vec<FunctionDef>, Vec<StructDef>, Vec<String>) {
    let mut functions = Vec::new();
    let mut structs = Vec::new();
    let mut imports = Vec::new();

    // Function declarations: (export)? (async)? function name(params)
    let fn_decl_re = Regex::new(
        r"(?m)^[ \t]*(export\s+)?(default\s+)?(async\s+)?function\s+(\w+)\s*(?:<[^>]*>)?\s*\(([^)]*)\)"
    ).unwrap();

    // Arrow functions: (export)? (const|let) name = (async)? (params) =>
    // [^=\n]* absorbs optional type annotation after name and after params
    // (return type may contain `>` from generics, so we allow all chars except `=` and newline).
    let arrow_re = Regex::new(
        r"(?m)^[ \t]*(export\s+)?(?:const|let)\s+(\w+)[^=\n]*=\s*(async\s+)?\(([^)]*)\)[^=\n]*=>"
    ).unwrap();

    // Interface declarations
    let interface_re = Regex::new(
        r"(?m)^[ \t]*(export\s+)?interface\s+(\w+)"
    ).unwrap();

    // Class declarations
    let class_re = Regex::new(
        r"(?m)^[ \t]*(export\s+)?(?:abstract\s+)?class\s+(\w+)"
    ).unwrap();

    // Type alias declarations
    let type_re = Regex::new(
        r"(?m)^[ \t]*(export\s+)?type\s+(\w+)\s*(?:<[^>]*)?\s*="
    ).unwrap();

    // Named imports: import { X, Y } from 'pkg'
    let import_named_re = Regex::new(
        r#"(?m)^[ \t]*import\s+\{([^}]*)\}\s+from\s+['"]([^'"]+)['"]"#
    ).unwrap();

    // Default imports: import X from 'pkg'
    let import_default_re = Regex::new(
        r#"(?m)^[ \t]*import\s+(\w+)\s+from\s+['"]([^'"]+)['"]"#
    ).unwrap();

    // Namespace imports: import * as X from 'pkg'
    let import_ns_re = Regex::new(
        r#"(?m)^[ \t]*import\s+\*\s+as\s+\w+\s+from\s+['"]([^'"]+)['"]"#
    ).unwrap();

    // Side-effect imports: import 'pkg'
    let import_side_re = Regex::new(
        r#"(?m)^[ \t]*import\s+['"]([^'"]+)['"]"#
    ).unwrap();

    // Field pattern inside interfaces/classes: "  fieldName?:" or "  fieldName:"
    let field_re = Regex::new(r"^\s{2,}(\w+)\s*[?:]").unwrap();

    // ── Functions ──────────────────────────────────────────────────────────

    for cap in fn_decl_re.captures_iter(source) {
        let is_pub = cap.get(1).is_some(); // has `export`
        let is_async = cap.get(3).is_some();
        let raw_name = cap[4].to_string();
        let params_raw = cap[5].trim().to_string();
        let params: Vec<String> = split_params(&params_raw);
        let name = normalize_name(&raw_name);
        let line = line_of(source, cap.get(0).unwrap().start());
        functions.push(FunctionDef { name, is_public: is_pub, is_async, params, return_type: None, line });
    }

    for cap in arrow_re.captures_iter(source) {
        let is_pub = cap.get(1).is_some();
        let raw_name = cap[2].to_string();
        let is_async = cap.get(3).is_some();
        let params_raw = cap[4].trim().to_string();
        let params: Vec<String> = split_params(&params_raw);
        let name = normalize_name(&raw_name);
        let line = line_of(source, cap.get(0).unwrap().start());
        functions.push(FunctionDef { name, is_public: is_pub, is_async, params, return_type: None, line });
    }

    // ── Types: interfaces ─────────────────────────────────────────────────

    for cap in interface_re.captures_iter(source) {
        let name = cap[2].to_string();
        let line = line_of(source, cap.get(0).unwrap().start());
        let field_count = count_block_fields(source, cap.get(0).unwrap().end(), &field_re);
        structs.push(StructDef {
            name,
            is_public: cap.get(1).is_some(),
            fields: (0..field_count).map(|j| format!("field_{}", j)).collect(),
            derives: vec![],
            line,
        });
    }

    // ── Types: classes ───────────────────────────────────────────────────

    for cap in class_re.captures_iter(source) {
        let name = cap[2].to_string();
        let line = line_of(source, cap.get(0).unwrap().start());
        let field_count = count_block_fields(source, cap.get(0).unwrap().end(), &field_re);
        structs.push(StructDef {
            name,
            is_public: cap.get(1).is_some(),
            fields: (0..field_count).map(|j| format!("field_{}", j)).collect(),
            derives: vec![],
            line,
        });
    }

    // ── Types: type aliases ───────────────────────────────────────────────

    for cap in type_re.captures_iter(source) {
        let name = cap[2].to_string();
        let line = line_of(source, cap.get(0).unwrap().start());
        structs.push(StructDef {
            name,
            is_public: cap.get(1).is_some(),
            fields: vec![],
            derives: vec![],
            line,
        });
    }

    // ── Imports ───────────────────────────────────────────────────────────

    // Named imports
    for cap in import_named_re.captures_iter(source) {
        let pkg = cap[2].trim();
        let names = cap[1].trim();
        let ext = is_external_ts(pkg);
        for name in names.split(',') {
            let name = name.trim().split(" as ").next().unwrap_or("").trim();
            if !name.is_empty() {
                let path = format!("{}/{}", pkg, name);
                imports.push(if ext { format!("ext:{}", path) } else { path });
            }
        }
    }

    // Default imports
    for cap in import_default_re.captures_iter(source) {
        let pkg = cap[2].trim();
        let ext = is_external_ts(pkg);
        imports.push(if ext { format!("ext:{}", pkg) } else { pkg.to_string() });
    }

    // Namespace imports
    for cap in import_ns_re.captures_iter(source) {
        let pkg = cap[1].trim();
        let ext = is_external_ts(pkg);
        imports.push(if ext { format!("ext:{}", pkg) } else { pkg.to_string() });
    }

    // Side-effect imports
    for cap in import_side_re.captures_iter(source) {
        let pkg = cap[1].trim();
        let ext = is_external_ts(pkg);
        imports.push(if ext { format!("ext:{}", pkg) } else { pkg.to_string() });
    }

    imports.sort();
    imports.dedup();

    (functions, structs, imports)
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn line_of(source: &str, byte_offset: usize) -> usize {
    source[..byte_offset].chars().filter(|&c| c == '\n').count() + 1
}

fn split_params(raw: &str) -> Vec<String> {
    if raw.is_empty() {
        return vec![];
    }
    raw.split(',')
        .map(|p| {
            // Strip type annotations: "name: Type" → "name"
            p.split(':').next().unwrap_or(p).trim().to_string()
        })
        .filter(|s| !s.is_empty())
        .collect()
}

/// Count field-like lines inside the opening `{` block following `offset`.
fn count_block_fields(source: &str, offset: usize, field_re: &Regex) -> usize {
    let rest = &source[offset..];
    let brace_start = match rest.find('{') {
        Some(p) => p + 1,
        None => return 0,
    };
    let block = &rest[brace_start..];
    let mut depth = 1usize;
    let mut count = 0usize;
    for line in block.lines() {
        for ch in line.chars() {
            if ch == '{' { depth += 1; }
            if ch == '}' {
                depth -= 1;
                if depth == 0 { return count; }
            }
        }
        if depth == 1 && field_re.is_match(line) {
            count += 1;
        }
    }
    count
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
import { Pool } from 'pg';
import axios from 'axios';
import * as fs from 'fs';
import { Logger } from './logger';

export interface User {
  id: number;
  name: string;
  email: string;
}

export class UserService {
  private pool: Pool;
  private logger: Logger;

  constructor(pool: Pool) {
    this.pool = pool;
  }
}

export async function getUser(pool: Pool, id: string): Promise<User> {
  const res = await pool.query('SELECT * FROM users WHERE id = $1', [id]);
  return res.rows[0];
}

export const createUser = async (pool: Pool, data: Partial<User>): Promise<User> => {
  const res = await pool.query('INSERT INTO users ...', [data.name, data.email]);
  return res.rows[0];
};

function internalHelper(x: number): number {
  return x * 2;
}
"#;

    #[test]
    fn ts_functions() {
        let (fns, _, _) = parse_typescript(SAMPLE);
        let names: Vec<&str> = fns.iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"get_user"), "get_user not found: {:?}", names);
        assert!(names.contains(&"create_user"), "create_user not found: {:?}", names);
        assert!(names.contains(&"internal_helper"), "internal_helper not found: {:?}", names);
    }

    #[test]
    fn ts_classes_and_interfaces() {
        let (_, types, _) = parse_typescript(SAMPLE);
        let names: Vec<&str> = types.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"User"), "User not found: {:?}", names);
        assert!(names.contains(&"UserService"), "UserService not found: {:?}", names);
    }

    #[test]
    fn ts_interface_field_count() {
        let (_, types, _) = parse_typescript(SAMPLE);
        let user = types.iter().find(|t| t.name == "User").expect("User not found");
        assert!(user.fields.len() >= 3, "User should have >=3 fields, got {:?}", user.fields.len());
    }

    #[test]
    fn ts_imports_external() {
        let (_, _, imports) = parse_typescript(SAMPLE);
        assert!(imports.iter().any(|i| i.starts_with("ext:") && i.contains("pg")),
            "pg should be external: {:?}", imports);
        assert!(imports.iter().any(|i| !i.starts_with("ext:") && i.contains("fs")),
            "fs (Node stdlib) should not be external: {:?}", imports);
    }

    #[test]
    fn ts_async_detection() {
        let (fns, _, _) = parse_typescript(SAMPLE);
        let gu = fns.iter().find(|f| f.name == "get_user").expect("get_user missing");
        assert!(gu.is_async, "get_user should be async");
    }

    #[test]
    fn ts_visibility() {
        let (fns, _, _) = parse_typescript(SAMPLE);
        let gu = fns.iter().find(|f| f.name == "get_user").expect("get_user missing");
        assert!(gu.is_public, "get_user (export) should be public");
        let ih = fns.iter().find(|f| f.name == "internal_helper").expect("internal_helper missing");
        assert!(!ih.is_public, "internal_helper (no export) should not be public");
    }
}
