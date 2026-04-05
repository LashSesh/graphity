// Copyright (c) 2026 Sebastian Klemm
// SPDX-License-Identifier: MIT
//
//! Regex-based Go parser for ISLS I4.
//!
//! Extracts function/method declarations, struct/interface type declarations,
//! and import statements from `.go` files. Visibility is inferred from the
//! first character of the identifier (uppercase = exported/public).

use regex::Regex;
use crate::{FunctionDef, StructDef};
use crate::normalize::normalize_name;

// ─── Visibility helper ────────────────────────────────────────────────────────

/// Returns `true` when the Go identifier starts with an uppercase letter
/// (i.e. is exported / public).
pub fn is_pub_go(name: &str) -> bool {
    name.chars().next().map(|c| c.is_uppercase()).unwrap_or(false)
}

// ─── Import helper ────────────────────────────────────────────────────────────

/// Returns `true` when the import path refers to an external module.
///
/// Go convention: paths containing a `.` (like `github.com/...`) are external;
/// short names without a dot (`fmt`, `os`, `net/http`) are stdlib.
pub fn is_external_go(path: &str) -> bool {
    path.contains('.')
}

// ─── Parser ───────────────────────────────────────────────────────────────────

/// Parse Go source and return (functions, types, imports).
///
/// Import strings: stdlib modules kept as-is (e.g. `"fmt"`);
/// external modules prefixed with `ext:` (e.g. `"ext:github.com/pkg/errors"`).
pub fn parse_go(source: &str) -> (Vec<FunctionDef>, Vec<StructDef>, Vec<String>) {
    let mut functions = Vec::new();
    let mut structs = Vec::new();
    let mut imports = Vec::new();

    // func declarations — with optional receiver: func (r *Recv) Name(params) rettype
    let func_re = Regex::new(
        r"(?m)^func\s+(?:\([^)]+\)\s+)?(\w+)\s*\(([^)]*)\)"
    ).unwrap();

    // type X struct {
    let type_struct_re = Regex::new(r"(?m)^type\s+(\w+)\s+struct\s*\{").unwrap();

    // type X interface {
    let type_iface_re = Regex::new(r"(?m)^type\s+(\w+)\s+interface\s*\{").unwrap();

    // Single-line import: import "path"
    let import_single_re = Regex::new(r#"(?m)^import\s+"([^"]+)""#).unwrap();

    // Field pattern inside struct body (non-empty line not starting with //)
    let field_re = Regex::new(r"^\s+\w").unwrap();

    // ── Functions ─────────────────────────────────────────────────────────

    for cap in func_re.captures_iter(source) {
        let raw_name = cap[1].to_string();
        let params_raw = cap[2].trim().to_string();
        let is_pub = is_pub_go(&raw_name);
        let name = normalize_name(&raw_name);

        // Count params: split on comma, each "name type" pair counts as 1
        let params: Vec<String> = if params_raw.is_empty() {
            vec![]
        } else {
            params_raw
                .split(',')
                .map(|p| p.trim().split_whitespace().next().unwrap_or("").to_string())
                .filter(|s| !s.is_empty())
                .collect()
        };

        let line = line_of(source, cap.get(0).unwrap().start());
        // Go has no async keyword — goroutines are implicit
        functions.push(FunctionDef {
            name,
            is_public: is_pub,
            is_async: false,
            params,
            return_type: None,
            line,
        });
    }

    // ── Types: structs ────────────────────────────────────────────────────

    for cap in type_struct_re.captures_iter(source) {
        let name = cap[1].to_string();
        let line = line_of(source, cap.get(0).unwrap().start());
        let is_pub = is_pub_go(&name);
        let field_count = count_struct_fields(source, cap.get(0).unwrap().end(), &field_re);
        structs.push(StructDef {
            name,
            is_public: is_pub,
            fields: (0..field_count).map(|j| format!("field_{}", j)).collect(),
            derives: vec![],
            line,
        });
    }

    // ── Types: interfaces ─────────────────────────────────────────────────

    for cap in type_iface_re.captures_iter(source) {
        let name = cap[1].to_string();
        let line = line_of(source, cap.get(0).unwrap().start());
        let is_pub = is_pub_go(&name);
        structs.push(StructDef {
            name,
            is_public: is_pub,
            fields: vec![],
            derives: vec!["interface".to_string()],
            line,
        });
    }

    // ── Imports ───────────────────────────────────────────────────────────

    // Single-line imports
    for cap in import_single_re.captures_iter(source) {
        let path = cap[1].trim().to_string();
        push_import(&mut imports, &path);
    }

    // Import blocks: import ( ... )
    let block_re = Regex::new(r#"(?ms)import\s*\(([^)]+)\)"#).unwrap();
    let item_re = Regex::new(r#""([^"]+)""#).unwrap();
    for block_cap in block_re.captures_iter(source) {
        let block = &block_cap[1];
        for item_cap in item_re.captures_iter(block) {
            let path = item_cap[1].trim().to_string();
            push_import(&mut imports, &path);
        }
    }

    imports.sort();
    imports.dedup();

    (functions, structs, imports)
}

fn push_import(imports: &mut Vec<String>, path: &str) {
    if is_external_go(path) {
        imports.push(format!("ext:{}", path));
    } else {
        imports.push(path.to_string());
    }
}

fn line_of(source: &str, byte_offset: usize) -> usize {
    source[..byte_offset].chars().filter(|&c| c == '\n').count() + 1
}

/// Count non-empty, non-comment lines inside the `{…}` block immediately
/// following `offset`. These represent struct fields.
fn count_struct_fields(source: &str, offset: usize, field_re: &Regex) -> usize {
    let rest = &source[offset..];
    let mut depth = 1usize; // we are already past the opening `{`
    let mut count = 0usize;
    for line in rest.lines() {
        for ch in line.chars() {
            if ch == '{' { depth += 1; }
            if ch == '}' {
                depth -= 1;
                if depth == 0 { return count; }
            }
        }
        let trimmed = line.trim();
        if depth == 1
            && !trimmed.is_empty()
            && !trimmed.starts_with("//")
            && field_re.is_match(line)
        {
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
package main

import (
    "fmt"
    "os"
    "github.com/jackc/pgx/v4/pgxpool"
    "github.com/google/uuid"
)

type User struct {
    ID    uuid.UUID
    Name  string
    Email string
}

type UserRepository interface {
    FindByID(id uuid.UUID) (*User, error)
    Create(u *User) error
}

func GetUser(pool *pgxpool.Pool, id uuid.UUID) (*User, error) {
    var u User
    return &u, nil
}

func CreateUser(pool *pgxpool.Pool, u *User) error {
    return nil
}

func (r *userRepo) findInternal(id string) (*User, error) {
    return nil, fmt.Errorf("not found")
}

func init() {
    fmt.Println(os.Getenv("HOME"))
}
"#;

    #[test]
    fn go_functions() {
        let (fns, _, _) = parse_go(SAMPLE);
        let names: Vec<&str> = fns.iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"get_user"), "get_user not found: {:?}", names);
        assert!(names.contains(&"create_user"), "create_user not found: {:?}", names);
    }

    #[test]
    fn go_structs_field_count() {
        let (_, types, _) = parse_go(SAMPLE);
        let user = types.iter().find(|t| t.name == "User").expect("User struct not found");
        assert_eq!(user.fields.len(), 3, "User should have 3 fields: {:?}", user.fields);
    }

    #[test]
    fn go_imports_external() {
        let (_, _, imports) = parse_go(SAMPLE);
        assert!(imports.iter().any(|i| i.starts_with("ext:") && i.contains("pgx")),
            "pgx should be external: {:?}", imports);
        assert!(imports.iter().any(|i| !i.starts_with("ext:") && i.contains("fmt")),
            "fmt should be stdlib: {:?}", imports);
    }

    #[test]
    fn go_async_always_false() {
        let (fns, _, _) = parse_go(SAMPLE);
        assert!(fns.iter().all(|f| !f.is_async), "Go functions should never be async");
    }

    #[test]
    fn go_visibility_uppercase() {
        let (fns, _, _) = parse_go(SAMPLE);
        let get_user = fns.iter().find(|f| f.name == "get_user").expect("get_user missing");
        assert!(get_user.is_public, "GetUser (uppercase) should be public");

        let find_internal = fns.iter().find(|f| f.name == "find_internal");
        if let Some(f) = find_internal {
            assert!(!f.is_public, "findInternal (lowercase) should not be public");
        }
    }

    #[test]
    fn go_interface_detected() {
        let (_, types, _) = parse_go(SAMPLE);
        let iface = types.iter().find(|t| t.name == "UserRepository")
            .expect("UserRepository interface not found");
        assert!(iface.derives.contains(&"interface".to_string()));
    }
}
