// Copyright (c) 2026 Sebastian Klemm
// SPDX-License-Identifier: MIT
//
//! Name normalization utilities for cross-language Resonite extraction (I4).
//!
//! Converts camelCase and PascalCase names to snake_case, and strips
//! language-specific return-type wrappers so that functions from Rust,
//! Python, TypeScript and Go map to the same normalised form.

// ─── Name normalisation ───────────────────────────────────────────────────────

/// Normalise a function / method name to snake_case.
///
/// - `getUser`   → `get_user`
/// - `GetUser`   → `get_user`
/// - `get_user`  → `get_user`  (unchanged)
/// - `HTTPServer`→ `h_t_t_p_server` (best-effort; only splits on case boundaries)
pub fn normalize_name(name: &str) -> String {
    let mut result = String::new();
    let chars: Vec<char> = name.chars().collect();
    for (i, &ch) in chars.iter().enumerate() {
        if ch.is_uppercase() && i > 0 {
            let prev = chars[i - 1];
            // Insert underscore when: lowercase→Uppercase or digit→Uppercase
            if prev.is_lowercase() || prev.is_ascii_digit() {
                result.push('_');
            } else if i + 1 < chars.len() && chars[i + 1].is_lowercase() && prev.is_uppercase() {
                // e.g. "HTMLParser": H-T-M-L → insert before L
                result.push('_');
            }
        }
        result.push(ch.to_lowercase().next().unwrap_or(ch));
    }
    result
}

// ─── Return-type unwrapping ───────────────────────────────────────────────────

/// Wrapper type names whose inner type should be extracted.
const WRAPPERS: &[&str] = &[
    "Result", "Option", "Promise", "Optional", "Future",
    "Box", "Arc", "Rc", "Pin", "Stream", "Vec",
];

/// Strip language-specific return-type wrappers and return the core type.
///
/// - `Result<User>`    → `User`
/// - `Promise<User>`   → `User`
/// - `Optional[User]`  → `User`
/// - `(*User, error)`  → `User`
/// - `Option<String>`  → `String`
/// - Already bare      → returned as-is
pub fn unwrap_return_type(t: &str) -> String {
    let t = t.trim();

    // Go tuple: (*T, error) or (T, error) or (*T, error)
    if t.starts_with('(') && t.ends_with(')') {
        let inner = &t[1..t.len() - 1];
        // Take the first comma-separated element
        let first = inner.split(',').next().unwrap_or("").trim();
        let cleaned = first.trim_start_matches('*').trim();
        if !cleaned.is_empty() && cleaned.to_lowercase() != "error" {
            return unwrap_return_type(cleaned);
        }
        return t.to_string();
    }

    // Generic wrappers: Wrapper<Inner> or Wrapper[Inner]
    for &wrapper in WRAPPERS {
        if let Some(rest) = t.strip_prefix(wrapper) {
            if let Some(inner) = rest.strip_prefix('<').or_else(|| rest.strip_prefix('[')) {
                if let Some(inner) = inner.strip_suffix('>').or_else(|| inner.strip_suffix(']')) {
                    let inner = inner.trim();
                    if !inner.is_empty() {
                        return unwrap_return_type(inner);
                    }
                }
            }
        }
    }

    t.to_string()
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camel_case_to_snake() {
        assert_eq!(normalize_name("camelCase"), "camel_case");
    }

    #[test]
    fn pascal_case_to_snake() {
        assert_eq!(normalize_name("PascalCase"), "pascal_case");
    }

    #[test]
    fn snake_case_unchanged() {
        assert_eq!(normalize_name("snake_case"), "snake_case");
    }

    #[test]
    fn unwrap_result() {
        assert_eq!(unwrap_return_type("Result<User>"), "User");
    }

    #[test]
    fn unwrap_promise() {
        assert_eq!(unwrap_return_type("Promise<User>"), "User");
    }

    #[test]
    fn unwrap_optional_python() {
        assert_eq!(unwrap_return_type("Optional[User]"), "User");
    }

    #[test]
    fn unwrap_go_tuple() {
        assert_eq!(unwrap_return_type("(*User, error)"), "User");
    }

    #[test]
    fn unwrap_option() {
        assert_eq!(unwrap_return_type("Option<String>"), "String");
    }

    #[test]
    fn bare_type_unchanged() {
        assert_eq!(unwrap_return_type("User"), "User");
    }
}
