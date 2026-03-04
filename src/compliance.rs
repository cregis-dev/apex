use crate::config::{Compliance, PiiAction, PiiRule};
use once_cell::sync::Lazy;
use regex::Regex;
use tracing::warn;

/// Pre-built PII patterns
static BUILTIN_PATTERNS: Lazy<Vec<PiiRule>> = Lazy::new(|| {
    vec![
        PiiRule {
            name: "email".to_string(),
            pattern: r"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}".to_string(),
            action: PiiAction::Mask,
            mask_char: '*',
            replace_with: None,
        },
        PiiRule {
            name: "phone".to_string(),
            pattern: r"(\+?1[-.\s]?)?\(?[0-9]{3}\)?[-.\s]?[0-9]{3}[-.\s]?[0-9]{4}".to_string(),
            action: PiiAction::Mask,
            mask_char: '*',
            replace_with: None,
        },
        PiiRule {
            name: "credit_card".to_string(),
            pattern: r"\b(?:\d{4}[- ]?){3}\d{4}\b".to_string(),
            action: PiiAction::Block,
            mask_char: '*',
            replace_with: Some("[CREDIT_CARD]".to_string()),
        },
        PiiRule {
            name: "ip_address".to_string(),
            pattern: r"\b(?:(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\.){3}(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\b".to_string(),
            action: PiiAction::Mask,
            mask_char: '*',
            replace_with: None,
        },
    ]
});

/// PII detection result
#[derive(Debug, Clone)]
pub struct PiiDetection {
    pub rule_name: String,
    pub matched_text: String,
    pub action: PiiAction,
}

/// PII Processor for detecting and masking sensitive data
pub struct PiiProcessor {
    compiled_rules: Vec<(PiiRule, Regex)>,
}

impl PiiProcessor {
    /// Create a new PII processor with built-in rules and custom rules
    pub fn new(compliance: &Option<Compliance>) -> Self {
        let mut compiled_rules = Vec::new();

        // Add built-in rules
        for rule in BUILTIN_PATTERNS.iter() {
            if let Ok(regex) = Regex::new(&rule.pattern) {
                compiled_rules.push((rule.clone(), regex));
            }
        }

        // Add custom rules from config
        if let Some(compliance_config) = compliance
            && compliance_config.enabled
        {
            for rule in &compliance_config.rules {
                match Regex::new(&rule.pattern) {
                    Ok(regex) => {
                        compiled_rules.push((rule.clone(), regex));
                    }
                    Err(e) => {
                        warn!(
                            rule = %rule.name,
                            error = %e,
                            "Failed to compile PII regex pattern, skipping rule"
                        );
                    }
                }
            }
        }

        Self { compiled_rules }
    }

    /// Check if PII masking is enabled
    pub fn is_enabled(&self) -> bool {
        !self.compiled_rules.is_empty()
    }

    /// Process text and return masked version and any detections
    pub fn process(&self, text: &str) -> (String, Vec<PiiDetection>) {
        let mut detections = Vec::new();
        let mut result = text.to_string();

        for (rule, regex) in &self.compiled_rules {
            let matches: Vec<_> = regex.find_iter(text).collect();

            if !matches.is_empty() {
                for m in matches {
                    let matched_text = m.as_str().to_string();

                    detections.push(PiiDetection {
                        rule_name: rule.name.clone(),
                        matched_text: matched_text.clone(),
                        action: rule.action.clone(),
                    });

                    // Apply masking or replacement
                    let replacement = if let Some(ref replace_with) = rule.replace_with {
                        replace_with.clone()
                    } else {
                        let len = matched_text.len();
                        rule.mask_char.to_string().repeat(len)
                    };

                    result = result.replace(&matched_text, &replacement);
                }
            }
        }

        (result, detections)
    }

    /// Check if text contains any PII that should be blocked
    pub fn should_block(&self, text: &str) -> Option<PiiDetection> {
        for (rule, regex) in &self.compiled_rules {
            if rule.action == PiiAction::Block
                && let Some(m) = regex.find(text)
            {
                return Some(PiiDetection {
                    rule_name: rule.name.clone(),
                    matched_text: m.as_str().to_string(),
                    action: rule.action.clone(),
                });
            }
        }
        None
    }
}

/// Process JSON content and mask PII in all string values
pub fn process_json_content(
    processor: &PiiProcessor,
    json_str: &str,
) -> (String, Vec<PiiDetection>) {
    let mut all_detections = Vec::new();

    // Try to parse as JSON and process each string value
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(json_str) {
        let processed = process_json_value(&value, processor, &mut all_detections);
        if let Ok(result) = serde_json::to_string(&processed) {
            return (result, all_detections);
        }
    }

    // Fallback: process as plain text
    processor.process(json_str)
}

fn process_json_value(
    value: &serde_json::Value,
    processor: &PiiProcessor,
    detections: &mut Vec<PiiDetection>,
) -> serde_json::Value {
    match value {
        serde_json::Value::String(s) => {
            let (masked, new_detections) = processor.process(s);
            detections.extend(new_detections);
            serde_json::Value::String(masked)
        }
        serde_json::Value::Array(arr) => serde_json::Value::Array(
            arr.iter()
                .map(|v| process_json_value(v, processor, detections))
                .collect(),
        ),
        serde_json::Value::Object(obj) => serde_json::Value::Object(
            obj.iter()
                .map(|(k, v)| (k.clone(), process_json_value(v, processor, detections)))
                .collect(),
        ),
        _ => value.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_email_masking() {
        let processor = PiiProcessor::new(&None);
        let (result, detections) = processor.process("Contact: john.doe@example.com");

        // Email should be masked (original contains 16 chars, so result should have 16 asterisks)
        assert!(!result.contains("john.doe@example.com"));
        assert!(!detections.is_empty());
        assert_eq!(detections[0].rule_name, "email");
    }

    #[test]
    fn test_phone_masking() {
        let processor = PiiProcessor::new(&None);
        let (result, detections) = processor.process("Call me at (555) 123-4567");

        // Phone number should be masked
        assert!(!result.contains("(555) 123-4567"));
        assert!(!detections.is_empty());
    }

    #[test]
    fn test_credit_card_block() {
        let processor = PiiProcessor::new(&None);
        let should_block = processor.should_block("Card: 1234-5678-9012-3456");

        assert!(should_block.is_some());
        assert_eq!(should_block.unwrap().rule_name, "credit_card");
    }

    #[test]
    fn test_custom_rule() {
        let custom_rule = PiiRule {
            name: "ssn".to_string(),
            pattern: r"\d{3}-\d{2}-\d{4}".to_string(),
            action: PiiAction::Mask,
            mask_char: '*',
            replace_with: None,
        };

        let compliance = Compliance {
            enabled: true,
            rules: vec![custom_rule],
        };

        let processor = PiiProcessor::new(&Some(compliance));
        let (result, detections) = processor.process("SSN: 123-45-6789");

        // SSN should be masked
        assert!(!result.contains("123-45-6789"));
        assert!(!detections.is_empty());
        assert_eq!(detections[0].rule_name, "ssn");
    }

    #[test]
    fn test_process_json_content() {
        let processor = PiiProcessor::new(&None);
        let json = r#"{"message": "Contact john@example.com for info"}"#;

        let (result, detections) = process_json_content(&processor, json);

        assert!(!result.contains("john@example.com"));
        assert!(!detections.is_empty());
    }
}
