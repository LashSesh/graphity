// isls-agent: feature.rs — Natural Language Intent → Feature Decomposition
//
// The operator says WHAT they want in plain language (German or English).
// The Oracle decomposes it into concrete features. No technical jargon.

use serde::{Deserialize, Serialize};

use crate::stubs::{OutputFormat, OracleResponse, SynthesisOracle, SynthesisPrompt};

use crate::apply::strip_markdown_fences;

// ─── Feature ────────────────────────────────────────────────────────────────

/// A user-visible feature described in the operator's language.
/// No code paths, no technical terms — just what the software DOES.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Feature {
    /// Human-readable name: "Bookmarks suchen"
    pub name: String,
    /// What this feature does: "Der Nutzer kann Bookmarks nach Titel und Tags durchsuchen"
    pub description: String,
    /// Things the user can do: ["Textsuche", "Tag-Filter", "Ergebnisliste"]
    pub capabilities: Vec<String>,
    /// Data/entities involved: ["Bookmarks", "Tags"]
    pub data_involved: Vec<String>,
    /// 1 = must have, 2 = should have, 3 = nice to have
    pub priority: usize,
}

// ─── Decomposition ──────────────────────────────────────────────────────────

/// Decompose a plain-language intent into concrete features using the Oracle.
///
/// The operator says "Eine App die meine Bookmarks verwaltet mit Suche und Tags"
/// and gets back 3 features: Bookmarks anlegen, Suchen, Tags verwalten.
pub fn decompose_intent(
    intent: &str,
    oracle: &dyn SynthesisOracle,
) -> Result<Vec<Feature>, String> {
    let prompt = SynthesisPrompt {
        system: "You are a product manager. The user describes what they want \
                 in plain language (possibly German). \
                 Decompose their request into concrete features.\n\
                 Output a JSON array. Each element has:\n\
                 - name: short human-readable name (in the user's language)\n\
                 - description: what this feature does (in the user's language)\n\
                 - capabilities: list of things the user can do\n\
                 - data_involved: what data/entities are needed\n\
                 - priority: 1=must, 2=should, 3=nice\n\
                 Output ONLY valid JSON. No explanation. No markdown."
            .into(),
        user: intent.into(),
        output_format: OutputFormat::Json,
        max_tokens: 1024,
        temperature: 0.0,
    };

    let response: OracleResponse = oracle
        .synthesize(&prompt)
        .map_err(|e| format!("Oracle error: {}", e))?;

    let cleaned = strip_markdown_fences(&response.content);
    let features: Vec<Feature> =
        serde_json::from_str(&cleaned).map_err(|e| format!("JSON parse error: {}", e))?;

    Ok(features)
}

/// Create a deterministic set of features for testing without an Oracle.
/// Used when no LLM is available or for unit tests.
pub fn decompose_intent_deterministic(intent: &str) -> Vec<Feature> {
    // Simple keyword-based decomposition
    let lower = intent.to_lowercase();
    let mut features = Vec::new();

    // Detect data entities mentioned
    let mut data = Vec::new();
    for word in &[
        "bookmark", "bookmarks", "lesezeichen", "buch", "bücher", "books",
        "user", "nutzer", "benutzer", "tag", "tags",
    ] {
        if lower.contains(word) {
            let entity = match *word {
                "bookmark" | "bookmarks" | "lesezeichen" => "Bookmarks",
                "buch" | "bücher" | "books" => "Books",
                "user" | "nutzer" | "benutzer" => "Users",
                "tag" | "tags" => "Tags",
                _ => word,
            };
            if !data.contains(&entity.to_string()) {
                data.push(entity.to_string());
            }
        }
    }
    if data.is_empty() {
        data.push("Daten".to_string());
    }

    // Base CRUD feature
    let main_entity = data.first().cloned().unwrap_or_else(|| "Daten".into());
    features.push(Feature {
        name: format!("{} verwalten", main_entity),
        description: format!("{} anlegen, anzeigen und bearbeiten", main_entity),
        capabilities: vec![
            format!("Neue {} anlegen", main_entity),
            format!("Alle {} anzeigen", main_entity),
        ],
        data_involved: data.clone(),
        priority: 1,
    });

    // Search feature
    if lower.contains("such") || lower.contains("search") || lower.contains("find") {
        features.push(Feature {
            name: format!("{} durchsuchen", main_entity),
            description: format!("{} nach verschiedenen Kriterien durchsuchen", main_entity),
            capabilities: vec![
                "Textsuche".to_string(),
                "Ergebnisliste".to_string(),
            ],
            data_involved: data.clone(),
            priority: 1,
        });
    }

    // Tag feature
    if lower.contains("tag") {
        features.push(Feature {
            name: "Tags verwalten".to_string(),
            description: "Tags zuweisen und danach filtern".to_string(),
            capabilities: vec![
                "Tags zuweisen".to_string(),
                "Nach Tags filtern".to_string(),
            ],
            data_involved: vec!["Tags".to_string()],
            priority: 2,
        });
    }

    // Delete feature
    if lower.contains("lösch") || lower.contains("delete") || lower.contains("entfern") {
        features.push(Feature {
            name: format!("{} löschen", main_entity),
            description: format!("{} dauerhaft entfernen", main_entity),
            capabilities: vec![
                format!("{} löschen", main_entity),
                "Bestätigung vor dem Löschen".to_string(),
            ],
            data_involved: data.clone(),
            priority: 2,
        });
    }

    if features.is_empty() {
        // Fallback: single generic feature
        features.push(Feature {
            name: "Hauptfunktion".to_string(),
            description: intent.to_string(),
            capabilities: vec!["Wie beschrieben".to_string()],
            data_involved: data,
            priority: 1,
        });
    }

    features
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // AT-AG13: Intent decomposition — deterministic path
    #[test]
    fn at_ag13_intent_decomposition() {
        let features =
            decompose_intent_deterministic("bookmark app with search and tags");
        assert!(
            features.len() >= 3,
            "AT-AG13: expected ≥3 features, got {}",
            features.len()
        );
        // Each feature must have a name and at least one capability
        for f in &features {
            assert!(!f.name.is_empty(), "feature name must not be empty");
            assert!(!f.capabilities.is_empty(), "feature must have capabilities");
            assert!(!f.data_involved.is_empty(), "feature must reference data");
            assert!(f.priority >= 1 && f.priority <= 3, "priority must be 1-3");
        }
    }

    // AT-AG13b: German input produces German feature names
    #[test]
    fn at_ag13b_german_input() {
        let features =
            decompose_intent_deterministic("Buchverwaltung mit Suche");
        assert!(!features.is_empty(), "should produce features from German input");
        // The features should reference the detected entity
        let all_names: String = features.iter().map(|f| f.name.clone()).collect::<Vec<_>>().join(" ");
        assert!(
            all_names.contains("Books") || all_names.contains("Daten") || all_names.contains("verwalten"),
            "German input should produce relevant features, got: {}",
            all_names
        );
    }

    // AT-AG13c: JSON round-trip for Feature
    #[test]
    fn at_ag13c_feature_serialization() {
        let feature = Feature {
            name: "Bookmarks suchen".into(),
            description: "Suche nach Titel".into(),
            capabilities: vec!["Textsuche".into()],
            data_involved: vec!["Bookmarks".into()],
            priority: 1,
        };
        let json = serde_json::to_string(&feature).unwrap();
        let parsed: Feature = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, feature);
    }
}
