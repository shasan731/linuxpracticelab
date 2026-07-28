//! The validator registry, embedded from `lessons/schema/validators.json` at compile time.
//!
//! Both sides of the control channel use this: the host rejects a malformed lesson package
//! before sending anything to the guest, and the guest rejects a malformed request before
//! touching the system. Because it is one file compiled into one crate, the two cannot drift.
//!
//! The registry also drives a coverage test in the agent: every validator marked
//! `implemented` must appear in the dispatch table, so adding an entry here without wiring it
//! up fails CI rather than silently passing a learner's task.

use crate::lesson::Validator;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::OnceLock;

const REGISTRY_JSON: &str = include_str!("../../../lessons/schema/validators.json");

#[derive(Debug, Deserialize)]
pub struct Registry {
    #[serde(rename = "schemaVersion")]
    pub schema_version: u32,
    #[serde(rename = "failureCategories")]
    pub failure_categories: Vec<String>,
    #[serde(rename = "commonParams")]
    pub common_params: BTreeMap<String, ParamSpec>,
    pub validators: BTreeMap<String, ValidatorSpec>,
}

#[derive(Debug, Deserialize)]
pub struct ValidatorSpec {
    pub category: String,
    pub implemented: bool,
    #[serde(rename = "failureCategory")]
    pub failure_category: String,
    pub summary: String,
    #[serde(default)]
    pub params: BTreeMap<String, ParamSpec>,
    /// At least one of these parameter names must be present, e.g. a bound for `file_size`.
    #[serde(default, rename = "requiresOneOf")]
    pub requires_one_of: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct ParamSpec {
    #[serde(rename = "type")]
    pub param_type: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub values: Vec<String>,
    #[serde(default)]
    pub summary: Option<String>,
}

static REGISTRY: OnceLock<Registry> = OnceLock::new();

/// The embedded registry. Panics only if the compiled-in JSON is malformed, which a unit
/// test in this module rules out at build time.
pub fn registry() -> &'static Registry {
    REGISTRY.get_or_init(|| {
        serde_json::from_str(REGISTRY_JSON).expect("embedded validators.json must parse")
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryError {
    UnknownValidator(String),
    NotImplemented(String),
    MissingParam {
        kind: String,
        param: String,
    },
    UnknownParam {
        kind: String,
        param: String,
    },
    MissingOneOf {
        kind: String,
        options: Vec<String>,
    },
    WrongParamType {
        kind: String,
        param: String,
        expected: String,
    },
    BadEnumValue {
        kind: String,
        param: String,
        allowed: Vec<String>,
    },
}

impl std::fmt::Display for RegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownValidator(k) => write!(f, "unknown validator '{k}'"),
            Self::NotImplemented(k) => write!(
                f,
                "validator '{k}' is declared but not implemented by this agent"
            ),
            Self::MissingParam { kind, param } => {
                write!(f, "validator '{kind}' requires parameter '{param}'")
            }
            Self::UnknownParam { kind, param } => {
                write!(f, "validator '{kind}' does not accept parameter '{param}'")
            }
            Self::MissingOneOf { kind, options } => write!(
                f,
                "validator '{kind}' needs at least one of: {}",
                options.join(", ")
            ),
            Self::WrongParamType {
                kind,
                param,
                expected,
            } => write!(
                f,
                "validator '{kind}' parameter '{param}' must be of type {expected}"
            ),
            Self::BadEnumValue {
                kind,
                param,
                allowed,
            } => write!(
                f,
                "validator '{kind}' parameter '{param}' must be one of: {}",
                allowed.join(", ")
            ),
        }
    }
}

impl std::error::Error for RegistryError {}

impl Registry {
    pub fn spec(&self, kind: &str) -> Option<&ValidatorSpec> {
        self.validators.get(kind)
    }

    pub fn implemented_validators(&self) -> impl Iterator<Item = &String> {
        self.validators
            .iter()
            .filter(|(_, spec)| spec.implemented)
            .map(|(name, _)| name)
    }

    /// Checks one validator against the registry: known tag, implemented, no unknown
    /// parameters, required parameters present, types and enum values plausible.
    pub fn check(&self, validator: &Validator) -> Result<(), RegistryError> {
        let spec = self
            .spec(&validator.kind)
            .ok_or_else(|| RegistryError::UnknownValidator(validator.kind.clone()))?;
        if !spec.implemented {
            return Err(RegistryError::NotImplemented(validator.kind.clone()));
        }

        // Report misspelled/unsupported parameters before the missing parameter they may
        // have been intended to supply. "paht is unknown" is actionable; "path is missing"
        // alone can hide the authoring error that would otherwise be silently ignored.
        for name in validator.params.keys() {
            if !spec.params.contains_key(name) && !self.common_params.contains_key(name) {
                return Err(RegistryError::UnknownParam {
                    kind: validator.kind.clone(),
                    param: name.clone(),
                });
            }
        }

        for (name, param) in &spec.params {
            if param.required && !validator.params.contains_key(name) {
                return Err(RegistryError::MissingParam {
                    kind: validator.kind.clone(),
                    param: name.clone(),
                });
            }
        }

        if !spec.requires_one_of.is_empty()
            && !spec
                .requires_one_of
                .iter()
                .any(|name| validator.params.contains_key(name))
        {
            return Err(RegistryError::MissingOneOf {
                kind: validator.kind.clone(),
                options: spec.requires_one_of.clone(),
            });
        }

        for (name, value) in &validator.params {
            let param = spec
                .params
                .get(name)
                .or_else(|| self.common_params.get(name))
                .expect("unknown parameters were rejected above");
            check_value(&validator.kind, name, param, value)?;
        }

        Ok(())
    }
}

fn check_value(
    kind: &str,
    param_name: &str,
    spec: &ParamSpec,
    value: &serde_json::Value,
) -> Result<(), RegistryError> {
    let wrong = |expected: &str| RegistryError::WrongParamType {
        kind: kind.to_string(),
        param: param_name.to_string(),
        expected: expected.to_string(),
    };

    match spec.param_type.as_str() {
        "int" | "port" => {
            let n = value.as_i64().ok_or_else(|| wrong("an integer"))?;
            if spec.param_type == "port" && !(1..=65535).contains(&n) {
                return Err(wrong("a port between 1 and 65535"));
            }
        }
        "bool" => {
            value.as_bool().ok_or_else(|| wrong("a boolean"))?;
        }
        "list<string>" => {
            let items = value
                .as_array()
                .ok_or_else(|| wrong("an array of strings"))?;
            if items.iter().any(|i| !i.is_string()) {
                return Err(wrong("an array of strings"));
            }
        }
        "validators" => {
            let items = value
                .as_array()
                .ok_or_else(|| wrong("an array of validators"))?;
            for item in items {
                let nested: Validator = serde_json::from_value(item.clone())
                    .map_err(|_| wrong("an array of validators"))?;
                registry().check(&nested)?;
            }
        }
        "enum" => {
            let s = value.as_str().ok_or_else(|| wrong("a string"))?;
            if !spec.values.iter().any(|allowed| allowed == s) {
                return Err(RegistryError::BadEnumValue {
                    kind: kind.to_string(),
                    param: param_name.to_string(),
                    allowed: spec.values.clone(),
                });
            }
        }
        "path" => {
            let s = value.as_str().ok_or_else(|| wrong("a string path"))?;
            if s.is_empty() {
                return Err(wrong("a non-empty path"));
            }
        }
        "mode" => {
            // Accept both 0644 style strings and JSON numbers, since lesson authors write both.
            match value {
                serde_json::Value::String(s) => {
                    u32::from_str_radix(s.trim_start_matches("0o"), 8)
                        .map_err(|_| wrong("an octal mode such as \"0644\""))?;
                }
                serde_json::Value::Number(_) => {}
                _ => return Err(wrong("an octal mode such as \"0644\"")),
            }
        }
        "sha256" => {
            let s = value.as_str().ok_or_else(|| wrong("a hex digest"))?;
            if s.len() != 64 || !s.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(wrong("a 64 character hex SHA-256 digest"));
            }
        }
        // string, regex, user, group, unit, command and anything else: any string is legal.
        _ => {
            value.as_str().ok_or_else(|| wrong("a string"))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_registry_parses_and_is_populated() {
        let r = registry();
        assert_eq!(r.schema_version, 1);
        assert!(
            r.validators.len() >= 55,
            "expected the full validator set, got {}",
            r.validators.len()
        );
        assert_eq!(r.failure_categories.len(), 20);
    }

    #[test]
    fn every_declared_failure_category_is_a_known_category() {
        let r = registry();
        for (name, spec) in &r.validators {
            assert!(
                r.failure_categories.contains(&spec.failure_category),
                "{name} declares unknown failure category {}",
                spec.failure_category
            );
        }
    }

    #[test]
    fn every_validator_documents_itself() {
        for (name, spec) in &registry().validators {
            assert!(!spec.summary.is_empty(), "{name} has no summary");
            assert!(!spec.category.is_empty(), "{name} has no category");
        }
    }

    #[test]
    fn requires_one_of_names_real_params() {
        for (name, spec) in &registry().validators {
            for option in &spec.requires_one_of {
                assert!(
                    spec.params.contains_key(option),
                    "{name} requiresOneOf mentions unknown param {option}"
                );
            }
        }
    }

    #[test]
    fn enum_params_declare_their_allowed_values() {
        for (name, spec) in &registry().validators {
            for (param, p) in &spec.params {
                if p.param_type == "enum" {
                    assert!(
                        !p.values.is_empty(),
                        "{name}.{param} is an enum with no values"
                    );
                }
            }
        }
    }

    #[test]
    fn accepts_a_well_formed_validator() {
        let v = Validator::new("file_mode")
            .with("path", "/etc/report-api/config.env")
            .with("mode", "0640");
        assert!(registry().check(&v).is_ok());
    }

    #[test]
    fn rejects_unknown_tags_rather_than_ignoring_them() {
        let v = Validator::new("definitely_not_a_validator").with("path", "/tmp/x");
        assert_eq!(
            registry().check(&v),
            Err(RegistryError::UnknownValidator(
                "definitely_not_a_validator".into()
            ))
        );
    }

    #[test]
    fn rejects_a_missing_required_param() {
        let v = Validator::new("file_exists");
        assert_eq!(
            registry().check(&v),
            Err(RegistryError::MissingParam {
                kind: "file_exists".into(),
                param: "path".into()
            })
        );
    }

    #[test]
    fn rejects_a_typo_in_a_param_name() {
        // A silently ignored `paht` would make the check pass for the wrong reason.
        let v = Validator::new("file_exists").with("paht", "/home/student/a.txt");
        assert!(matches!(
            registry().check(&v),
            Err(RegistryError::UnknownParam { .. })
        ));
    }

    #[test]
    fn accepts_common_params_on_any_validator() {
        let v = Validator::new("file_exists")
            .with("path", "/home/student/a.txt")
            .with("message", "Create the report first.")
            .with("weight", 3)
            .with("failureCategory", "wrong_path");
        assert!(registry().check(&v).is_ok(), "{:?}", registry().check(&v));
    }

    #[test]
    fn file_size_needs_at_least_one_bound() {
        let bare = Validator::new("file_size").with("path", "/tmp/x");
        assert!(matches!(
            registry().check(&bare),
            Err(RegistryError::MissingOneOf { .. })
        ));
        let bounded = Validator::new("file_size")
            .with("path", "/tmp/x")
            .with("min", 1);
        assert!(registry().check(&bounded).is_ok());
    }

    #[test]
    fn enum_values_are_checked() {
        let bad = Validator::new("interface_state")
            .with("interface", "eth0")
            .with("state", "sideways");
        assert!(matches!(
            registry().check(&bad),
            Err(RegistryError::BadEnumValue { .. })
        ));
    }

    #[test]
    fn param_types_are_checked() {
        let bad = Validator::new("port_listening").with("port", "eighty");
        assert!(matches!(
            registry().check(&bad),
            Err(RegistryError::WrongParamType { .. })
        ));
        let out_of_range = Validator::new("port_listening").with("port", 70_000);
        assert!(matches!(
            registry().check(&out_of_range),
            Err(RegistryError::WrongParamType { .. })
        ));
    }

    #[test]
    fn nested_validators_are_checked_recursively() {
        let ok = Validator::new("side_effect_exists")
            .with("command", "./backup.sh")
            .with(
                "then",
                serde_json::json!([{ "type": "file_exists", "path": "/backup/latest.tar.gz" }]),
            );
        assert!(registry().check(&ok).is_ok());

        let bad = Validator::new("side_effect_exists")
            .with("command", "./backup.sh")
            .with("then", serde_json::json!([{ "type": "file_exists" }]));
        assert!(matches!(
            registry().check(&bad),
            Err(RegistryError::MissingParam { .. })
        ));
    }

    #[test]
    fn mode_accepts_octal_strings_and_rejects_nonsense() {
        let ok = Validator::new("file_mode")
            .with("path", "/tmp/x")
            .with("mode", "0755");
        assert!(registry().check(&ok).is_ok());
        let bad = Validator::new("file_mode")
            .with("path", "/tmp/x")
            .with("mode", "rwxr-xr-x");
        assert!(matches!(
            registry().check(&bad),
            Err(RegistryError::WrongParamType { .. })
        ));
    }
}
