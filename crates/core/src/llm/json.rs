use crate::error::{AgentError, Result};

/// Extract the first JSON object or array from LLM output.
///
/// Models often wrap JSON in code fences or prose; this finds the first
/// balanced `{...}` / `[...]` region (string-aware) and parses it.
pub fn extract_json(text: &str) -> Result<serde_json::Value> {
    if let Ok(value) = serde_json::from_str(text.trim()) {
        return Ok(value);
    }
    let candidate = balanced_json_slice(text)
        .ok_or_else(|| AgentError::LlmResponse(format!("no JSON found in: {}", preview(text))))?;
    serde_json::from_str(candidate)
        .map_err(|e| AgentError::LlmResponse(format!("invalid JSON ({e}): {}", preview(candidate))))
}

fn balanced_json_slice(text: &str) -> Option<&str> {
    let start = text.find(['{', '['])?;
    let bytes = text.as_bytes();
    let open = bytes[start];
    let close = if open == b'{' { b'}' } else { b']' };

    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (offset, &byte) in bytes[start..].iter().enumerate() {
        if in_string {
            match byte {
                _ if escaped => escaped = false,
                b'\\' => escaped = true,
                b'"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b if b == open => depth += 1,
            b if b == close => {
                depth -= 1;
                if depth == 0 {
                    return Some(&text[start..=start + offset]);
                }
            }
            _ => {}
        }
    }
    None
}

fn preview(text: &str) -> String {
    let trimmed = text.trim();
    let mut end = trimmed.len().min(200);
    while !trimmed.is_char_boundary(end) {
        end -= 1;
    }
    trimmed[..end].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_json() {
        let value = extract_json(r#"{"a": 1}"#).unwrap();
        assert_eq!(value["a"], 1);
    }

    #[test]
    fn parses_json_inside_code_fence() {
        let text = "Here you go:\n```json\n{\"queries\": [\"rust async\"]}\n```\nDone.";
        let value = extract_json(text).unwrap();
        assert_eq!(value["queries"][0], "rust async");
    }

    #[test]
    fn parses_array_with_surrounding_prose() {
        let value = extract_json("results: [1, 2, 3] as requested").unwrap();
        assert_eq!(value, serde_json::json!([1, 2, 3]));
    }

    #[test]
    fn handles_braces_inside_strings() {
        let value = extract_json(r#"{"note": "uses { and } inside"} trailing"#).unwrap();
        assert_eq!(value["note"], "uses { and } inside");
    }

    #[test]
    fn rejects_text_without_json() {
        assert!(extract_json("no structured data here").is_err());
    }
}
