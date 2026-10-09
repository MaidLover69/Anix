//! A plugin's `userConfig`: fields the user can set, whose values replace `{{fieldName}}`
//! placeholders in the plugin's code before it runs and answer `$getUserPreference`.
//!
//! The rules are Seanime's (`internal/extension_repo/userconfig.go`): with saved values each
//! field's placeholder is replaced by its saved value, or by its default when none was saved for
//! it; without saved values the defaults are used, unless the manifest says the plugin requires
//! configuration, in which case it is not loaded; saved values of another configuration version
//! are not used and the plugin is not loaded until they are saved again. Values are put into the
//! code as they are, without any escaping, one field after the other in manifest order.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

const MAX_FIELDS: usize = 64;
const MAX_NAME_CHARS: usize = 64;
const MAX_VALUE_CHARS: usize = 4096;
const MAX_OPTIONS: usize = 64;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ConfigOption {
    pub value: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ConfigField {
    pub name: String,
    pub label: String,
    /// `text`, `switch` or `select`.
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub default: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<ConfigOption>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct UserConfig {
    pub version: i64,
    #[serde(default)]
    pub requires_config: bool,
    pub fields: Vec<ConfigField>,
}

/// What the user saved for a plugin.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct SavedConfig {
    /// The configuration version the values were saved for.
    pub version: i64,
    pub values: HashMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigError {
    /// The plugin requires configuration and none was saved.
    Missing,
    /// The saved values are for another version of the plugin's configuration.
    Incompatible,
}

impl ConfigError {
    pub fn message(&self) -> &'static str {
        match self {
            ConfigError::Missing => "This plugin has to be configured before it can be used.",
            ConfigError::Incompatible => {
                "This plugin's settings changed with its update; review and save them again."
            }
        }
    }
}

fn clipped(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

/// A manifest value that should be a string; numbers and booleans are taken by their text.
fn text_of(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => clipped(text, MAX_VALUE_CHARS),
        Some(Value::Number(number)) => number.to_string(),
        Some(Value::Bool(flag)) => flag.to_string(),
        _ => String::new(),
    }
}

impl UserConfig {
    /// Reads `userConfig` from a manifest. Fields without a name are left out.
    pub fn from_manifest(manifest: &Value) -> Option<Self> {
        let config = manifest.get("userConfig").filter(|config| config.is_object())?;
        let mut fields: Vec<ConfigField> = Vec::new();
        for field in config.get("fields").and_then(Value::as_array).into_iter().flatten() {
            let name = text_of(field.get("name"));
            if name.is_empty() || name.chars().count() > MAX_NAME_CHARS || fields.iter().any(|f| f.name == name) {
                continue;
            }
            if fields.len() >= MAX_FIELDS {
                break;
            }
            let options = field
                .get("options")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .take(MAX_OPTIONS)
                .map(|option| ConfigOption {
                    value: text_of(option.get("value")),
                    label: text_of(option.get("label")),
                })
                .collect();
            let label = text_of(field.get("label"));
            fields.push(ConfigField {
                label: if label.is_empty() { name.clone() } else { label },
                kind: match field.get("type").and_then(Value::as_str) {
                    Some("switch") => "switch",
                    Some("select") => "select",
                    _ => "text",
                }
                .to_string(),
                default: text_of(field.get("default")),
                options,
                name,
            });
        }
        Some(Self {
            version: config.get("version").and_then(Value::as_i64).unwrap_or(0),
            requires_config: config.get("requiresConfig").and_then(Value::as_bool).unwrap_or(false),
            fields,
        })
    }

    /// The value each field has with these saved values: the saved one, or the default.
    pub fn values(&self, saved: Option<&SavedConfig>) -> Vec<(String, String)> {
        self.fields
            .iter()
            .map(|field| {
                let value = saved
                    .and_then(|saved| saved.values.get(&field.name))
                    .cloned()
                    .unwrap_or_else(|| field.default.clone());
                (field.name.clone(), value)
            })
            .collect()
    }

    /// What to save for values the user entered: values of unknown fields are dropped, the
    /// version is this configuration's.
    pub fn saved_from(&self, values: &HashMap<String, Value>) -> SavedConfig {
        let values = self
            .fields
            .iter()
            .filter_map(|field| {
                let value = values.get(&field.name).filter(|value| !value.is_null())?;
                Some((field.name.clone(), text_of(Some(value))))
            })
            .collect();
        SavedConfig {
            version: self.version,
            values,
        }
    }
}

/// The plugin code with its placeholders replaced, and the values `$getUserPreference` answers
/// with. A plugin without `userConfig` is returned unchanged.
pub fn apply(
    payload: &str,
    config: Option<&UserConfig>,
    saved: Option<&SavedConfig>,
) -> Result<(String, HashMap<String, String>), ConfigError> {
    let Some(config) = config else {
        return Ok((payload.to_string(), HashMap::new()));
    };
    match saved {
        None if config.requires_config => return Err(ConfigError::Missing),
        Some(saved) if saved.version != config.version => return Err(ConfigError::Incompatible),
        _ => {}
    }
    let mut code = payload.to_string();
    let mut values = HashMap::new();
    for (name, value) in config.values(saved) {
        let placeholder = format!("{{{{{}}}}}", name);
        if code.contains(&placeholder) {
            code = code.replace(&placeholder, &value);
        }
        values.insert(name, value);
    }
    Ok((code, values))
}
