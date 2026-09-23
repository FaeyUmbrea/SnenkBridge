//! SnenkBridge's saved formula language. Pest handles syntax; value and function
//! rules live here so a parser dependency change cannot change preset semantics.
mod functions;
mod value;

use pest::{
    iterators::Pair,
    pratt_parser::{Assoc, Op, PrattParser},
    Parser,
};
use pest_derive::Parser;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::LazyLock,
};
use value::{Result as EvalResult, Value};

#[derive(Parser)]
#[grammar = "src/evaluation/expression/formula.pest"]
struct FormulaParser;

static PRECEDENCE: LazyLock<PrattParser<Rule>> = LazyLock::new(|| {
    use Rule::*;
    PrattParser::new()
        .op(Op::infix(semicolon, Assoc::Left))
        .op(Op::infix(comma, Assoc::Left))
        .op(Op::infix(assignment, Assoc::Right))
        .op(Op::infix(or, Assoc::Left))
        .op(Op::infix(and, Assoc::Left))
        .op(Op::infix(equal, Assoc::Left)
            | Op::infix(unequal, Assoc::Left)
            | Op::infix(less, Assoc::Left)
            | Op::infix(less_equal, Assoc::Left)
            | Op::infix(greater, Assoc::Left)
            | Op::infix(greater_equal, Assoc::Left))
        .op(Op::infix(add, Assoc::Left) | Op::infix(subtract, Assoc::Left))
        .op(Op::infix(multiply, Assoc::Left)
            | Op::infix(divide, Assoc::Left)
            | Op::infix(modulo, Assoc::Left))
        .op(Op::prefix(negate) | Op::prefix(not))
        .op(Op::infix(power, Assoc::Left))
});

#[derive(Debug)]
enum Node {
    Constant(Value),
    Missing,
    // Remembers a leading binary operator that consumed adjacent operands.
    Filled(Box<Node>),
    Adjacent(Vec<Node>),
    Variable(String),
    Group(Box<Node>),
    Unary(Rule, Box<Node>),
    Binary(Rule, Box<Node>, Box<Node>),
    Sequence(Rule, Vec<Node>),
    Call(String, Box<Node>),
}

pub(super) struct Expression {
    root: Node,
    variables: BTreeSet<String>,
}

impl Expression {
    pub fn parse(source: &str) -> Result<Self, String> {
        let source = without_comments(source)?;
        let formula = FormulaParser::parse(Rule::formula, &source)
            .map_err(|e| e.to_string())?
            .next()
            .ok_or("missing formula")?;
        let root = build(formula.into_inner().next().ok_or("missing expression")?)?;
        let mut variables = BTreeSet::new();
        let mut pending = vec![&root];
        while let Some(node) = pending.pop() {
            match node {
                Node::Variable(name) => {
                    variables.insert(name.clone());
                }
                Node::Filled(n) | Node::Group(n) | Node::Unary(_, n) | Node::Call(_, n) => {
                    pending.push(n)
                }
                Node::Binary(_, a, b) => {
                    pending.push(a);
                    pending.push(b);
                }
                Node::Sequence(_, nodes) => pending.extend(nodes),
                Node::Adjacent(_) => return Err("missing operator between values".into()),
                Node::Constant(_) | Node::Missing => {}
            }
        }
        Ok(Self { root, variables })
    }
    pub fn variables(&self) -> impl Iterator<Item = &str> {
        self.variables.iter().map(String::as_str)
    }
    pub fn evaluate(&self, inputs: &BTreeMap<String, f64>) -> EvalResult<f64> {
        self.root.evaluate(inputs)?.number()
    }
}

fn literal(text: &str) -> Node {
    let integer = [("0x", 16), ("0b", 2), ("0o", 8)]
        .into_iter()
        .find_map(|(prefix, radix)| {
            text.strip_prefix(prefix)
                .map(|digits| i64::from_str_radix(digits, radix))
        })
        .unwrap_or_else(|| text.parse());
    Node::Constant(if let Ok(n) = integer {
        Value::Int(n)
    } else if let Ok(n) = text.parse::<f64>() {
        Value::Float(n)
    } else if let Ok(b) = text.parse::<bool>() {
        Value::Bool(b)
    } else {
        return Node::Variable(text.into());
    })
}

fn build(pair: Pair<'_, Rule>) -> Result<Node, String> {
    Ok(match pair.as_rule() {
        Rule::expression => {
            return PRECEDENCE
                .map_primary(build)
                .map_prefix(|op, operand| Ok(Node::Unary(op.as_rule(), Box::new(operand?))))
                .map_infix(|left, op, right| {
                    let (mut left, mut right) = (left?, right?);
                    if op.as_rule() == Rule::assignment
                        && matches!(left, Node::Missing)
                        && matches!(right, Node::Filled(_))
                    {
                        return Err("missing assignment target".into());
                    }
                    let mut filled = false;
                    if !matches!(op.as_rule(), Rule::comma | Rule::semicolon)
                        && matches!(left, Node::Missing)
                    {
                        if let Some(first) = right.take_leading_adjacent() {
                            left = first;
                            filled = true;
                        }
                    }
                    let rule = op.as_rule();
                    let trailing_empty = matches!(right, Node::Missing);
                    if matches!(rule, Rule::comma | Rule::semicolon) {
                        if matches!(left, Node::Missing) {
                            left = Node::Constant(Value::Empty);
                        }
                        if matches!(right, Node::Missing) {
                            right = Node::Constant(Value::Empty);
                        }
                    }
                    if rule == Rule::semicolon && matches!(left, Node::Sequence(Rule::comma, _)) {
                        return Err("a tuple before a semicolon needs parentheses".into());
                    }
                    // Preserve the saved language's treatment of a tuple in
                    // an already-started sequence: `1;2,3;4` ends in (2,3,4).
                    if rule == Rule::semicolon {
                        if let Node::Sequence(Rule::semicolon, items) = &mut left {
                            if matches!(items.last(), Some(Node::Sequence(Rule::comma, _))) {
                                let Node::Sequence(_, mut tuple) = items.pop().unwrap() else {
                                    unreachable!()
                                };
                                let joined = match right {
                                    Node::Sequence(Rule::comma, mut tail) => {
                                        tuple.push(tail.remove(0));
                                        tail.insert(0, Node::Sequence(Rule::comma, tuple));
                                        Node::Sequence(Rule::comma, tail)
                                    }
                                    other => {
                                        if !trailing_empty {
                                            tuple.push(other);
                                        }
                                        Node::Sequence(Rule::comma, tuple)
                                    }
                                };
                                items.push(Node::Sequence(Rule::semicolon, vec![joined]));
                                return Ok(left);
                            }
                        }
                    }
                    if matches!(rule, Rule::comma | Rule::semicolon) {
                        let mut items = match left {
                            Node::Sequence(previous, items) if previous == rule => items,
                            other => vec![other],
                        };
                        items.push(right);
                        Ok(Node::Sequence(rule, items))
                    } else {
                        let node = Node::Binary(rule, Box::new(left), Box::new(right));
                        Ok(if filled {
                            Node::Filled(Box::new(node))
                        } else {
                            node
                        })
                    }
                })
                .parse(pair.into_inner())
                .map(|node| {
                    if matches!(node, Node::Missing) {
                        Node::Constant(Value::Empty)
                    } else {
                        node
                    }
                });
        }
        Rule::primary => {
            let mut items = Vec::new();
            for child in pair.into_inner() {
                let node = build(child)?;
                if matches!(&node, Node::Group(n) if matches!(**n, Node::Constant(Value::Empty)))
                    && matches!(items.last(), Some(Node::Call(_, arg)) if !matches!(**arg, Node::Group(_)))
                {
                    continue;
                }
                match node {
                    Node::Adjacent(nodes) => items.extend(nodes),
                    other => items.push(other),
                }
            }
            if items.len() == 1 {
                items.pop().unwrap()
            } else {
                Node::Adjacent(items)
            }
        }
        Rule::group => Node::Group(Box::new(build(
            pair.into_inner().next().ok_or("missing group")?,
        )?)),
        Rule::literal => literal(pair.as_str()),
        Rule::empty => Node::Missing,
        Rule::string => {
            let text = pair.as_str();
            let mut chars = text[1..text.len() - 1].chars();
            let mut decoded = String::new();
            while let Some(c) = chars.next() {
                decoded.push(if c == '\\' {
                    chars.next().ok_or("missing escape")?
                } else {
                    c
                });
            }
            Node::Constant(Value::String(decoded))
        }
        Rule::call => {
            let mut pairs = pair.into_inner();
            let name = pairs.next().ok_or("missing function")?;
            let argument = build(pairs.next().ok_or("missing argument")?)?;
            match literal(name.as_str()) {
                Node::Variable(name) => Node::Call(name, Box::new(argument)),
                constant => Node::Adjacent(vec![constant, argument]),
            }
        }
        _ => return Err("unexpected formula token".into()),
    })
}

impl Node {
    // A leading binary operator can consume two adjacent operands in saved
    // formulas, e.g. `/ 8 2`. Keep its normal precedence when doing so.
    fn take_leading_adjacent(&mut self) -> Option<Node> {
        match self {
            Self::Adjacent(nodes) if nodes.len() >= 2 && !matches!(nodes[0], Self::Call(_, _)) => {
                let first = nodes.remove(0);
                if nodes.len() == 1 {
                    *self = nodes.pop().unwrap();
                }
                Some(first)
            }
            Self::Binary(_, left, right) => {
                if let Some(first) = left.take_leading_adjacent() {
                    return Some(first);
                }
                if !matches!(
                    **left,
                    Self::Call(_, _) | Self::Group(_) | Self::Binary(_, _, _) | Self::Unary(_, _)
                ) {
                    if let Some(first) = right.take_leading_adjacent() {
                        return Some(*std::mem::replace(left, Box::new(first)));
                    }
                }
                None
            }
            _ => None,
        }
    }
    fn evaluate(&self, inputs: &BTreeMap<String, f64>) -> EvalResult<Value> {
        match self {
            Self::Constant(v) => Ok(v.clone()),
            Self::Adjacent(_) | Self::Missing => Err(()),
            Self::Variable(name) => inputs
                .get(name)
                .copied()
                .filter(|n| n.is_finite())
                .map(Value::Float)
                .ok_or(()),
            Self::Filled(n) | Self::Group(n) => n.evaluate(inputs),
            Self::Unary(op, n) => n.evaluate(inputs)?.unary(*op),
            Self::Binary(Rule::assignment, _, _) => Err(()), // Tracking inputs are read-only.
            Self::Binary(op, a, b) => a.evaluate(inputs)?.binary(*op, b.evaluate(inputs)?),
            Self::Call(name, arg) => functions::call(name, arg.evaluate(inputs)?),
            Self::Sequence(op, nodes) => {
                let values = nodes
                    .iter()
                    .map(|n| n.evaluate(inputs))
                    .collect::<EvalResult<Vec<_>>>()?;
                if *op == Rule::comma {
                    Ok(Value::Tuple(values))
                } else {
                    Ok(values.into_iter().last().unwrap_or(Value::Empty))
                }
            }
        }
    }
}

// Comments are removed before tokenization: `1/* comment */2` is the integer 12.
// Quoted text and escaped quotes must survive this pass unchanged.
fn without_comments(source: &str) -> Result<String, String> {
    let mut chars = source.chars().peekable();
    let mut result = String::with_capacity(source.len());
    let mut quoted = false;
    while let Some(c) = chars.next() {
        if quoted {
            result.push(c);
            if c == '\\' {
                if let Some(escaped) = chars.next() {
                    result.push(escaped);
                }
            } else if c == '"' {
                quoted = false;
            }
        } else if c == '"' {
            quoted = true;
            result.push(c);
        } else if c == '/' && chars.peek() == Some(&'/') {
            chars.next();
            for c in chars.by_ref() {
                if c == '\n' {
                    break;
                }
            }
        } else if c == '/' && chars.peek() == Some(&'*') {
            chars.next();
            let mut closed = false;
            while let Some(c) = chars.next() {
                if c == '*' && chars.peek() == Some(&'/') {
                    chars.next();
                    closed = true;
                    break;
                }
            }
            if !closed {
                return Err("unclosed comment".into());
            }
        } else {
            result.push(c);
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formula_values_match_saved_language_contract() {
        // Recorded with evalexpr 13.1.0. Keeping data instead of an old
        // engine dependency lets dependency upgrades use the same contract.
        let cases: Vec<(String, bool, Option<String>)> =
            serde_json::from_str(include_str!("language_cases.json")).unwrap();
        let inputs = BTreeMap::from([("Input".into(), 0.5)]);
        for (source, valid, expected) in cases {
            let expression = Expression::parse(&source);
            assert_eq!(expression.is_ok(), valid, "syntax: {source}");
            let actual = expression
                .ok()
                .and_then(|e| e.root.evaluate(&inputs).ok())
                .map(|v| format!("{}:{}", v.kind(), v.text()));
            // Platform math libraries can round transcendental results a few
            // ULPs apart. Preserve the type and all nonnumeric results exactly.
            if let (Some(a), Some(b)) = (
                actual.as_deref().and_then(|s| s.strip_prefix("float:")),
                expected.as_deref().and_then(|s| s.strip_prefix("float:")),
            ) {
                let (a, b) = (a.parse::<f64>().unwrap(), b.parse::<f64>().unwrap());
                assert!(
                    a == b
                        || (a.is_nan() && b.is_nan())
                        || (a.is_finite()
                            && b.is_finite()
                            && (a - b).abs()
                                <= 32.0 * f64::EPSILON * b.abs().max(f64::MIN_POSITIVE)),
                    "value: {source}: {a} != {b}"
                );
            } else {
                assert_eq!(actual, expected, "value: {source}");
            }
        }
    }

    #[test]
    fn unsafe_numeric_and_string_operations_return_errors() {
        for source in [
            "math::abs(-9223372036854775807 - 1)",
            "-(-9223372036854775807 - 1)",
            "(-9223372036854775807 - 1) / -1",
            "(-9223372036854775807 - 1) % -1",
            "shl(1,64)",
            "shl(1,-1)",
            "shr(1,64)",
            "shr(1,-1)",
            "str::substring(\"ä\",1)",
            "str::substring(\"ä\",0,1)",
        ] {
            assert!(
                Expression::parse(source)
                    .unwrap()
                    .root
                    .evaluate(&BTreeMap::new())
                    .is_err(),
                "{source}"
            );
        }
    }
}
