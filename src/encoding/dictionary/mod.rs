use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DictionaryEncoded {
    pub dictionary: Vec<String>,
    pub codes: Vec<u32>,
}

pub fn encode(values: &[String]) -> DictionaryEncoded {
    let mut index_by_value = HashMap::new();
    let mut dictionary = Vec::new();
    let mut codes = Vec::with_capacity(values.len());

    for value in values {
        let code = match index_by_value.get(value) {
            Some(code) => *code,
            None => {
                let code = dictionary.len() as u32;
                dictionary.push(value.clone());
                index_by_value.insert(value.clone(), code);
                code
            }
        };
        codes.push(code);
    }

    DictionaryEncoded { dictionary, codes }
}

pub fn decode(encoded: &DictionaryEncoded) -> Vec<String> {
    encoded
        .codes
        .iter()
        .filter_map(|code| encoded.dictionary.get(*code as usize).cloned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dictionary_encoding_round_trip() {
        let values = vec![
            "open".to_string(),
            "closed".to_string(),
            "open".to_string(),
            "open".to_string(),
        ];
        let encoded = encode(&values);
        let decoded = decode(&encoded);

        assert_eq!(encoded.dictionary.len(), 2);
        assert_eq!(decoded, values);
    }
}
