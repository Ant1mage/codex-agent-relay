//! Parsing `agy models`, the CLI's own model catalogue.
//!
//! The output is a two-column table — the exact `--model` value, a tab, and the
//! human label. Relay only ever reports what the CLI printed: no names are
//! invented and no JSON flag is assumed (the CLI rejects `models
//! --output-format json`, so the plain form is the only one probed). A line that
//! is not an `id<TAB>label` row — a banner, an error, a log line — is not a
//! model and is dropped.

use relay_core::ModelOption;

/// The model rows the CLI listed, in the CLI's own order.
pub(super) fn model_catalogue(stdout: &str) -> Vec<ModelOption> {
    let mut models: Vec<ModelOption> = Vec::new();
    for line in stdout.lines() {
        let line = line.trim_end();
        if line.trim().is_empty() {
            continue;
        }
        // Strictly two columns: without the tab this is not a catalogue row, so
        // an error or any other arbitrary text is never mistaken for a model.
        let Some((value, label)) = line.split_once('\t') else {
            continue;
        };
        let value = value.trim();
        let label = label.trim();
        if value.is_empty() || label.is_empty() || is_header(value) {
            continue;
        }
        if models.iter().any(|model| model.value == value) {
            continue;
        }
        models.push(ModelOption::new(value.to_string(), Some(label.to_string())));
    }
    models
}

fn is_header(value: &str) -> bool {
    matches!(
        value.to_ascii_lowercase().as_str(),
        "model" | "models" | "id" | "name" | "model id" | "model name"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_models_table_is_parsed_without_inventing_rows() {
        let models = model_catalogue(
            "gemini-3.8-flash-high\tGemini 3.8 Flash (High)\n\
             gemini-3.8-flash-medium\tGemini 3.8 Flash (Medium)\n\
             claude-sonnet-4-6\tClaude Sonnet 4.6 (Thinking)\n",
        );
        assert_eq!(models.len(), 3);
        assert_eq!(models[0].value, "gemini-3.8-flash-high");
        assert_eq!(models[0].label.as_deref(), Some("Gemini 3.8 Flash (High)"));
        assert_eq!(models[2].value, "claude-sonnet-4-6");
    }

    #[test]
    fn headers_blanks_and_duplicates_are_dropped() {
        let models =
            model_catalogue("Available models:\nModel\tDescription\n\nm\tLabel\nm\tOther label\n");
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].value, "m");
        assert_eq!(models[0].label.as_deref(), Some("Label"));
    }

    /// A failed catalogue prints something other than `id<TAB>label`; none of it
    /// may become a selectable model.
    #[test]
    fn non_catalogue_text_is_never_a_model() {
        let models = model_catalogue(
            "error: not logged in\n\
             Usage: agy models\n\
             no models available\n\
             \t\n\
             \torphan label\n\
             lone-id-without-tab\n",
        );
        assert!(models.is_empty());
    }
}
