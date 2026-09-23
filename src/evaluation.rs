use crate::model::{DelaySettings, Parameter};
use std::collections::{BTreeMap, BTreeSet};

mod expression;
use expression::Expression;
#[cfg(test)]
mod tests;

const LIMIT: f64 = 1_000_000.0;
const MAX_HISTORY: usize = 1_000_000;
fn eval_expression(expression: &str) -> String {
    let mut result = expression.replace("math::pi()", &std::f64::consts::PI.to_string());
    for name in ["min", "max", "floor", "ceil"] {
        result = result.replace(&format!("math::{name}"), name);
    }
    result
}

fn parse_expression(expression: &str) -> Result<Expression, String> {
    Expression::parse(&eval_expression(expression))
}
struct Delay {
    settings: DelaySettings,
    history: Vec<f64>,
    cursor: usize,
    current: f64,
}
impl Delay {
    fn advance(&mut self, input: f64) -> f64 {
        let s = &self.settings;
        let normalized = if s.in_max == s.in_min {
            0.0
        } else {
            (input.clamp(s.in_min, s.in_max) - s.in_min) / (s.in_max - s.in_min)
        };
        self.history[self.cursor] = s.out_min + normalized * (s.out_max - s.out_min);
        self.cursor = (self.cursor + 1) % self.history.len();
        self.current += (self.history[self.cursor] - self.current) / s.smoothing.max(1.0);
        self.current
    }
}
struct Entry {
    parameter: Parameter,
    expression: Option<Expression>,
    delay: Option<Delay>,
}
pub struct Evaluator {
    entries: Vec<Entry>,
    order: Vec<usize>,
}
fn structural_errors(params: &[Parameter]) -> Vec<String> {
    let mut errors = Vec::new();
    let mut names = BTreeSet::new();
    for p in params {
        if p.name.trim().is_empty() || !names.insert(&p.name) {
            errors.push(format!("{}: empty or duplicate parameter name", p.name));
        }
        if ![p.min, p.max, p.default_value]
            .iter()
            .all(|v| v.is_finite())
            || p.min > p.max
            || p.default_value < p.min
            || p.default_value > p.max
        {
            errors.push(format!("{}: invalid range or default", p.name));
        }
        if let Some(d) = &p.delay_buffer {
            if ![d.in_min, d.in_max, d.out_min, d.out_max, d.smoothing]
                .iter()
                .all(|v| v.is_finite())
                || d.in_min > d.in_max
                || d.out_min > d.out_max
                || d.delay_count > MAX_HISTORY
            {
                errors.push(format!("{}: invalid delay range or history length", p.name));
            }
        }
    }
    errors
}
fn dependency_order(params: &[Parameter]) -> Result<Vec<usize>, String> {
    fn visit(
        i: usize,
        params: &[Parameter],
        state: &mut [u8],
        result: &mut Vec<usize>,
    ) -> Result<(), String> {
        if state[i] == 2 {
            return Ok(());
        }
        if state[i] == 1 {
            return Err(format!("{}: delayed dependency cycle", params[i].name));
        }
        state[i] = 1;
        if let Some(d) = &params[i].delay_buffer {
            if let Some(j) = params.iter().position(|p| p.name == d.ref_param) {
                visit(j, params, state, result)?;
            }
        }
        state[i] = 2;
        result.push(i);
        Ok(())
    }
    let mut result = Vec::new();
    let mut state = vec![0; params.len()];
    for i in 0..params.len() {
        visit(i, params, &mut state, &mut result)?;
    }
    Ok(result)
}
pub fn validation_errors(params: &[Parameter]) -> Vec<String> {
    let structural = structural_errors(params);
    let cycle = dependency_order(params).err();
    params
        .iter()
        .map(|p| {
            let mut errors: Vec<String> = structural
                .iter()
                .filter(|e| e.starts_with(&format!("{}:", p.name)))
                .cloned()
                .collect();
            if p.delay_buffer.is_none() {
                if p.func.trim().is_empty() {
                    errors.push("expression is empty".into());
                } else if let Err(e) = parse_expression(&p.func) {
                    errors.push(e.to_string());
                }
            }
            if let Some(e) = &cycle {
                if p.delay_buffer.is_some() {
                    errors.push(e.clone())
                }
            }
            errors.join("; ")
        })
        .collect()
}
pub fn time_variables(params: &[Parameter], elapsed_ms: u64) -> BTreeMap<String, f64> {
    let mut result = BTreeMap::new();
    for p in params {
        if let Ok(node) = parse_expression(&p.func) {
            for name in node.variables() {
                let period = name
                    .strip_prefix("Wave")
                    .or_else(|| name.strip_prefix("PingPong"))
                    .and_then(|n| n.parse::<u64>().ok())
                    .filter(|n| *n > 0);
                if let Some(period) = period {
                    let phase = (elapsed_ms % period) as f64 / period as f64;
                    result.insert(
                        name.to_owned(),
                        if name.starts_with("Wave") {
                            1.0 - (2.0 * phase - 1.0).abs()
                        } else {
                            phase
                        },
                    );
                }
            }
        }
    }
    result
}
impl Evaluator {
    pub fn new(params: &[Parameter]) -> Result<Self, String> {
        let errors = structural_errors(params);
        if !errors.is_empty() {
            return Err(errors.join("\n"));
        }
        let order = dependency_order(params)?;
        let entries = params
            .iter()
            .map(|p| Entry {
                parameter: p.clone(),
                expression: if p.func.trim().is_empty() {
                    None
                } else {
                    parse_expression(&p.func).ok()
                },
                delay: p.delay_buffer.clone().map(|settings| Delay {
                    history: vec![0.0; settings.delay_count.max(1)],
                    settings,
                    cursor: 0,
                    current: 0.0,
                }),
            })
            .collect();
        Ok(Self { entries, order })
    }
    pub fn evaluate(
        &mut self,
        values: &BTreeMap<String, f64>,
        advance: bool,
    ) -> BTreeMap<String, f64> {
        let mut output = BTreeMap::new();
        for &i in &self.order {
            let entry = &mut self.entries[i];
            let p = &entry.parameter;
            let value = if let Some(delay) = &mut entry.delay {
                if advance {
                    let input = output
                        .get(&delay.settings.ref_param)
                        .or_else(|| values.get(&delay.settings.ref_param))
                        .copied()
                        .filter(|v: &f64| v.is_finite())
                        .unwrap_or(0.0);
                    delay.advance(input)
                } else {
                    delay.current
                }
            } else if let Some(node) = &entry.expression {
                node.evaluate(values).unwrap_or(p.default_value)
            } else {
                continue;
            };
            output.insert(
                p.name.clone(),
                if value.is_finite() {
                    value.clamp(-LIMIT, LIMIT)
                } else {
                    p.default_value.clamp(-LIMIT, LIMIT)
                },
            );
        }
        output
    }
}
