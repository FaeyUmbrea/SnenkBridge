use std::collections::HashMap;

use evalexpr::{ContextWithMutableVariables, HashMapContext, Node};
use log::error;

use crate::vitamins::CalcFn;

/// A pre-compiled expression ready for evaluation.
pub struct CompiledParam {
    pub name: String,
    pub func: String,
    pub default_value: f64,
    pub(crate) node: Node,
}

/// Result of evaluating a single parameter expression.
pub struct EvalResult {
    pub name: String,
    pub value: f64,
}

/// Validate a single expression string.
///
/// Returns `None` when the expression is valid (or empty, which is treated as a
/// no-op that falls back to the default value), or `Some(message)` describing
/// the parse error.
#[must_use]
pub fn validate_expression(func: &str) -> Option<String> {
    if func.is_empty() {
        return None;
    }
    match evalexpr::build_operator_tree(func) {
        Ok(_) => None,
        Err(err) => Some(err.to_string()),
    }
}

/// Compile a list of `CalcFn` entries into evaluable expressions.
///
/// Skips entries that have a delay buffer set (those are handled separately
/// by the caller) and entries whose expression is empty or fails to parse.
pub fn compile_expressions(params: &[CalcFn]) -> Vec<CompiledParam> {
    let mut compiled = Vec::new();

    for param in params {
        // Skip delay-buffer parameters — caller handles them.
        if param.delay_buffer.is_some() {
            continue;
        }

        // Skip empty expressions.
        if param.func.is_empty() {
            continue;
        }

        let node = match evalexpr::build_operator_tree(&param.func) {
            Ok(n) => n,
            Err(err) => {
                error!(
                    "Skipping parameter '{}': invalid expression '{}': {}",
                    param.name, param.func, err
                );
                continue;
            }
        };

        compiled.push(CompiledParam {
            name: param.name.clone(),
            func: param.func.clone(),
            default_value: param.default_value,
            node,
        });
    }

    compiled
}

/// Evaluate all compiled expressions against the given variable values.
///
/// Each result is clamped to `[-1_000_000, 1_000_000]`. If evaluation fails
/// (e.g. a variable is missing from `values`) the `default_value` recorded in
/// the [`CompiledParam`] is used instead.
pub fn evaluate(compiled: &[CompiledParam], values: &HashMap<String, f64>) -> Vec<EvalResult> {
    let mut context = HashMapContext::new();
    for (k, v) in values {
        context.set_value(k.clone(), (*v).into()).unwrap();
    }

    compiled
        .iter()
        .map(|p| {
            let value = p
                .node
                .eval_with_context(&context)
                .ok()
                .and_then(|v| v.as_float().ok())
                .unwrap_or(p.default_value)
                .clamp(-1_000_000.0, 1_000_000.0);

            EvalResult {
                name: p.name.clone(),
                value,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vitamins::{CalcFn, DelayBuffer};

    fn make_param(name: &str, func: &str, default_value: f64) -> CalcFn {
        CalcFn {
            name: name.to_string(),
            func: func.to_string(),
            min: -1.0,
            max: 1.0,
            default_value,
            delay_buffer: None,
        }
    }

    #[test]
    fn evaluate_simple_passthrough() {
        let params = vec![make_param("JawOpen", "JawOpen", 0.0)];
        let compiled = compile_expressions(&params);
        assert_eq!(compiled.len(), 1);

        let mut values = HashMap::new();
        values.insert("JawOpen".to_string(), 0.75_f64);

        let results = evaluate(&compiled, &values);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "JawOpen");
        assert!((results[0].value - 0.75).abs() < 1e-9);
    }

    #[test]
    fn evaluate_arithmetic_expression() {
        let params = vec![make_param(
            "Result",
            "(JawOpen - MouthClose) * 0.5",
            0.0,
        )];
        let compiled = compile_expressions(&params);
        assert_eq!(compiled.len(), 1);

        let mut values = HashMap::new();
        values.insert("JawOpen".to_string(), 0.8_f64);
        values.insert("MouthClose".to_string(), 0.2_f64);

        let results = evaluate(&compiled, &values);
        assert_eq!(results.len(), 1);
        assert!((results[0].value - 0.3).abs() < 1e-9);
    }

    #[test]
    fn evaluate_clamps_extreme_values() {
        let params = vec![make_param("BigParam", "x * 9999999", 0.0)];
        let compiled = compile_expressions(&params);
        assert_eq!(compiled.len(), 1);

        let mut values = HashMap::new();
        values.insert("x".to_string(), 1.0_f64);

        let results = evaluate(&compiled, &values);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].value, 1_000_000.0);
    }

    #[test]
    fn skip_invalid_expressions() {
        let params = vec![
            make_param("Valid", "x * 2.0", 0.0),
            make_param("Invalid", "(((", 0.0),
        ];
        let compiled = compile_expressions(&params);
        assert_eq!(compiled.len(), 1);
        assert_eq!(compiled[0].name, "Valid");
    }

    #[test]
    fn skip_delay_buffer_params() {
        let delay_buf = DelayBuffer {
            ref_param: "Other".to_string(),
            smoothing: 0.5,
            delay_count: 3,
            in_min: 0.0,
            in_max: 1.0,
            out_min: 0.0,
            out_max: 1.0,
        };
        let params = vec![
            make_param("Normal", "x", 0.0),
            CalcFn {
                name: "Delayed".to_string(),
                func: String::new(),
                min: 0.0,
                max: 1.0,
                default_value: 0.0,
                delay_buffer: Some(delay_buf),
            },
        ];
        let compiled = compile_expressions(&params);
        assert_eq!(compiled.len(), 1);
        assert_eq!(compiled[0].name, "Normal");
    }

    #[test]
    fn validate_expression_reports_errors() {
        assert!(validate_expression("").is_none());
        assert!(validate_expression("(JawOpen - MouthClose) * 0.5").is_none());
        assert!(validate_expression("(((").is_some());
    }

    #[test]
    fn missing_variable_returns_default() {
        let params = vec![make_param("Param", "UndefinedVar * 2.0", 0.42)];
        let compiled = compile_expressions(&params);
        assert_eq!(compiled.len(), 1);

        // Empty context — variable is not present.
        let results = evaluate(&compiled, &HashMap::new());
        assert_eq!(results.len(), 1);
        assert!((results[0].value - 0.42).abs() < 1e-9);
    }
}
