//! JSON whose text does not depend on how `serde_json` was built.
//!
//! `serde_json`'s `preserve_order` feature makes a `Value` object keep its keys in insertion
//! order instead of sorted order. GPUI turns it on (ADR 0025), and cargo unifies features
//! across a build, so in any build that includes the GPUI UI (and in every whole-workspace test
//! run) the same `Value` is written with its keys in another order. Text that is compared or
//! stored, such as golden snapshots, goes through [`canonical_json`] first.

use serde_json::{Map, Value};

/// `value` with the keys of every object sorted, so it is written the same whether or not
/// `preserve_order` is on. Arrays keep their order.
pub fn canonical_json(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let mut sorted = Map::new();
            for k in keys {
                if let Some(v) = map.get(k) {
                    sorted.insert(k.clone(), canonical_json(v));
                }
            }
            Value::Object(sorted)
        }
        Value::Array(items) => Value::Array(items.iter().map(canonical_json).collect()),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn keys_are_sorted_at_every_level_and_arrays_keep_their_order() {
        let v = json!({ "zeta": 1, "alpha": { "y": [3, { "b": 2, "a": 1 }], "x": null } });
        assert_eq!(
            serde_json::to_string(&canonical_json(&v)).unwrap(),
            r#"{"alpha":{"x":null,"y":[3,{"a":1,"b":2}]},"zeta":1}"#
        );
    }
}
