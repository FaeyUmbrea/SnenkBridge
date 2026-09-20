use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default)]
pub struct TrackingFrame {
    pub values: BTreeMap<String, f64>,
    pub face_present: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DelaySettings {
    pub ref_param: String,
    pub smoothing: f64,
    pub delay_count: usize,
    pub in_min: f64,
    pub in_max: f64,
    pub out_min: f64,
    pub out_max: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Parameter {
    pub name: String,
    pub func: String,
    pub min: f64,
    pub max: f64,
    pub default_value: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delay_buffer: Option<DelaySettings>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Preset {
    pub format: String,
    pub version: u32,
    pub title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub author: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    pub params: Vec<Parameter>,
}
impl Preset {
    pub fn new(title: impl Into<String>, params: Vec<Parameter>) -> Self {
        Self {
            format: "snek".into(),
            version: 1,
            title: title.into(),
            author: String::new(),
            description: String::new(),
            params,
        }
    }
}
