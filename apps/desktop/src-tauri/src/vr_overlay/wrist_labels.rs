use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WristLabels(BTreeMap<String, String>);

impl Default for WristLabels {
    fn default() -> Self {
        let resource: serde_json::Value =
            serde_json::from_str(include_str!("../../../src/i18n/locales/en-US.json"))
                .expect("valid English locale");
        let mut labels: BTreeMap<String, String> =
            serde_json::from_value(resource["translation"]["settings"]["vrOcr"]["states"].clone())
                .expect("valid OCR state labels");
        let reader: BTreeMap<String, String> =
            serde_json::from_value(resource["translation"]["wristReader"].clone())
                .expect("valid wrist reader labels");
        labels.extend(reader);
        Self(labels)
    }
}

#[cfg(windows)]
impl WristLabels {
    pub fn text<'a>(&'a self, key: &'a str) -> &'a str {
        self.0
            .get(key)
            .filter(|text| !text.is_empty())
            .map(String::as_str)
            .unwrap_or(key)
    }
}
