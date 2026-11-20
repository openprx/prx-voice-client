//! Validation of provider parameters against a template's `params_schema` (UX 4.8.1).
//!
//! The schema is an object mapping a parameter name to `{ type, default, min, max }` with `type`
//! one of `number`, `integer`, `string`, `boolean`, `object`. Defaults are applied; a value of
//! the wrong type or outside `min` and `max` is refused. A key the schema does not list is passed
//! through (the editor allows custom keys); the adapter refuses the ones it owns.

use serde_json::{Map, Value};

/// A parameter that does not satisfy the schema.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParamError {
    /// The parameter name.
    pub param: String,
    /// What is wrong with it.
    pub reason: String,
}

fn type_matches(kind: &str, value: &Value) -> bool {
    match kind {
        "number" => value.is_number(),
        "integer" => value.is_i64() || value.is_u64(),
        "string" => value.is_string(),
        "boolean" => value.is_boolean(),
        "object" => value.is_object(),
        "array" => value.is_array(),
        _ => true,
    }
}

/// The schema defaults overlaid with `given`, or every violation found.
pub fn apply_schema(
    schema: Option<&Value>,
    given: &Map<String, Value>,
) -> Result<Map<String, Value>, Vec<ParamError>> {
    let mut out = Map::new();
    let mut errors = Vec::new();
    if let Some(Value::Object(fields)) = schema {
        for (name, spec) in fields {
            if let Some(default) = spec.get("default") {
                out.insert(name.clone(), default.clone());
            }
        }
        for (name, value) in given {
            let Some(spec) = fields.get(name) else {
                continue;
            };
            if let Some(kind) = spec.get("type").and_then(Value::as_str)
                && !type_matches(kind, value)
            {
                errors.push(ParamError {
                    param: name.clone(),
                    reason: format!("must be of type {kind}"),
                });
                continue;
            }
            if let Some(n) = value.as_f64() {
                let bound = |key: &str| spec.get(key).and_then(Value::as_f64);
                if let Some(min) = bound("min").filter(|min| n < *min) {
                    errors.push(ParamError {
                        param: name.clone(),
                        reason: format!("must be at least {min}"),
                    });
                } else if let Some(max) = bound("max").filter(|max| n > *max) {
                    errors.push(ParamError {
                        param: name.clone(),
                        reason: format!("must be at most {max}"),
                    });
                }
            }
        }
    }
    for (name, value) in given {
        out.insert(name.clone(), value.clone());
    }
    if errors.is_empty() { Ok(out) } else { Err(errors) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn schema() -> Value {
        json!({
            "temperature": {"type": "number", "default": 0, "min": 0, "max": 1},
            "cache_prompt": {"type": "boolean", "default": true},
            "idle_timeout_ms": {"type": "integer", "default": 3000, "min": 500, "max": 60000}
        })
    }

    fn given(v: Value) -> Map<String, Value> {
        v.as_object().cloned().unwrap()
    }

    #[test]
    fn defaults_apply_and_given_values_win() {
        let out = apply_schema(Some(&schema()), &given(json!({"temperature": 0.5, "custom": "x"}))).unwrap();
        assert_eq!(out["temperature"], json!(0.5));
        assert_eq!(out["cache_prompt"], json!(true));
        assert_eq!(out["idle_timeout_ms"], json!(3000));
        assert_eq!(out["custom"], json!("x"));
    }

    #[test]
    fn wrong_types_and_out_of_range_values_are_all_reported() {
        let errors = apply_schema(
            Some(&schema()),
            &given(json!({"temperature": 3, "cache_prompt": "yes", "idle_timeout_ms": 1.5})),
        )
        .unwrap_err();
        let names: Vec<_> = errors.iter().map(|e| e.param.as_str()).collect();
        assert_eq!(names.len(), 3);
        for n in ["temperature", "cache_prompt", "idle_timeout_ms"] {
            assert!(names.contains(&n), "{n}");
        }
    }

    #[test]
    fn no_schema_passes_everything_through() {
        let out = apply_schema(None, &given(json!({"a": 1}))).unwrap();
        assert_eq!(out["a"], json!(1));
    }
}
