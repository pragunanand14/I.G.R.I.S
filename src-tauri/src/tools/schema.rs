//! Minimal JSON-schema validation for tool inputs.
//!
//! Supports the subset IGRIS's tool schemas use: `object` with `properties`,
//! `required` and `additionalProperties: false`; `string` (`maxLength`,
//! `minLength`, `enum`); `number` / `integer` (`minimum`, `maximum`); `boolean`;
//! `array` (`items`, `maxItems`). Model-supplied input is untrusted, so tools
//! validate even when the provider claims strict schema adherence.

use serde_json::Value;

pub fn validate(schema: &Value, value: &Value) -> Result<(), String> {
    check(schema, value, "input")
}

fn check(schema: &Value, value: &Value, path: &str) -> Result<(), String> {
    let ty = schema["type"].as_str().unwrap_or("any");
    match ty {
        "object" => {
            let obj = value.as_object().ok_or_else(|| format!("{path} must be an object"))?;
            let props = schema["properties"].as_object();
            for req in schema["required"].as_array().into_iter().flatten().filter_map(Value::as_str) {
                if !obj.contains_key(req) {
                    return Err(format!("{path}.{req} is required"));
                }
            }
            for (k, v) in obj {
                match props.and_then(|p| p.get(k)) {
                    Some(s) => check(s, v, &format!("{path}.{k}"))?,
                    None if schema["additionalProperties"] == Value::Bool(false) => return Err(format!("{path}.{k} is not allowed")),
                    None => {}
                }
            }
        }
        "string" => {
            let s = value.as_str().ok_or_else(|| format!("{path} must be a string"))?;
            let n = s.chars().count() as u64;
            if let Some(max) = schema["maxLength"].as_u64() {
                if n > max {
                    return Err(format!("{path} must be at most {max} characters"));
                }
            }
            if let Some(min) = schema["minLength"].as_u64() {
                if n < min {
                    return Err(format!("{path} must be at least {min} characters"));
                }
            }
            if let Some(options) = schema["enum"].as_array() {
                if !options.iter().any(|o| o == value) {
                    return Err(format!("{path} must be one of {}", Value::Array(options.clone())));
                }
            }
        }
        "number" | "integer" => {
            let n = value.as_f64().ok_or_else(|| format!("{path} must be a number"))?;
            if ty == "integer" && !(value.is_i64() || value.is_u64()) {
                return Err(format!("{path} must be an integer"));
            }
            if schema["minimum"].as_f64().is_some_and(|m| n < m) || schema["maximum"].as_f64().is_some_and(|m| n > m) {
                return Err(format!("{path} is out of range"));
            }
        }
        "boolean" => {
            value.as_bool().ok_or_else(|| format!("{path} must be true or false"))?;
        }
        "array" => {
            let arr = value.as_array().ok_or_else(|| format!("{path} must be an array"))?;
            if schema["maxItems"].as_u64().is_some_and(|m| arr.len() as u64 > m) {
                return Err(format!("{path} has too many items"));
            }
            for (i, item) in arr.iter().enumerate() {
                check(&schema["items"], item, &format!("{path}[{i}]"))?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Strict tool use requires every object to forbid extra properties and to
/// list all of its properties as required.
pub fn is_strict_compatible(schema: &Value) -> bool {
    match schema["type"].as_str() {
        Some("object") => {
            if schema["additionalProperties"] != Value::Bool(false) {
                return false;
            }
            let props = schema["properties"].as_object().cloned().unwrap_or_default();
            let required: Vec<&str> = schema["required"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
            props.keys().all(|k| required.contains(&k.as_str())) && props.values().all(is_strict_compatible)
        }
        Some("array") => is_strict_compatible(&schema["items"]),
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn schema() -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {"type": "string", "minLength": 1, "maxLength": 5},
                "mode": {"type": "string", "enum": ["a", "b"]},
                "count": {"type": "integer", "minimum": 0, "maximum": 3},
                "tags": {"type": "array", "maxItems": 2, "items": {"type": "string"}}
            },
            "required": ["name", "mode", "count", "tags"],
            "additionalProperties": false
        })
    }

    #[test]
    fn accepts_valid_input() {
        assert!(validate(&schema(), &json!({"name":"abc","mode":"a","count":2,"tags":["x"]})).is_ok());
    }

    #[test]
    fn rejects_invalid_inputs() {
        let s = schema();
        for bad in [
            json!("not an object"),
            json!({"mode":"a","count":1,"tags":[]}),
            json!({"name":"toolong","mode":"a","count":1,"tags":[]}),
            json!({"name":"","mode":"a","count":1,"tags":[]}),
            json!({"name":"a","mode":"c","count":1,"tags":[]}),
            json!({"name":"a","mode":"a","count":1.5,"tags":[]}),
            json!({"name":"a","mode":"a","count":9,"tags":[]}),
            json!({"name":"a","mode":"a","count":1,"tags":["x","y","z"]}),
            json!({"name":"a","mode":"a","count":1,"tags":[1]}),
            json!({"name":"a","mode":"a","count":1,"tags":[],"shell":"rm -rf /"}),
        ] {
            assert!(validate(&s, &bad).is_err(), "should reject {bad}");
        }
    }

    #[test]
    fn strict_compatibility() {
        assert!(is_strict_compatible(&schema()));
        assert!(!is_strict_compatible(&json!({"type":"object","properties":{"a":{"type":"string"}},"required":[],"additionalProperties":false})));
        assert!(!is_strict_compatible(&json!({"type":"object","properties":{},"required":[]})));
    }
}
