use super::value::{Result, Value};

pub(super) fn call(name: &str, arg: Value) -> Result<Value> {
    use Value::*;
    let unary: Option<fn(f64) -> f64> = match name {
        "math::ln" => Some(f64::ln),
        "math::log2" => Some(f64::log2),
        "math::log10" => Some(f64::log10),
        "math::exp" => Some(f64::exp),
        "math::exp2" => Some(f64::exp2),
        "math::cos" => Some(f64::cos),
        "math::acos" => Some(f64::acos),
        "math::cosh" => Some(f64::cosh),
        "math::acosh" => Some(f64::acosh),
        "math::sin" => Some(f64::sin),
        "math::asin" => Some(f64::asin),
        "math::sinh" => Some(f64::sinh),
        "math::asinh" => Some(f64::asinh),
        "math::tan" => Some(f64::tan),
        "math::atan" => Some(f64::atan),
        "math::tanh" => Some(f64::tanh),
        "math::atanh" => Some(f64::atanh),
        "math::sqrt" => Some(f64::sqrt),
        "math::cbrt" => Some(f64::cbrt),
        "floor" => Some(f64::floor),
        "round" => Some(f64::round),
        "ceil" => Some(f64::ceil),
        _ => None,
    };
    if let Some(f) = unary {
        return Ok(Float(f(arg.number()?)));
    }
    match name {
        "math::log" | "math::pow" | "math::atan2" | "math::hypot" => {
            let [a, b] = arg.arguments()?;
            let (a, b) = (a.number()?, b.number()?);
            Ok(Float(match name {
                "math::log" => a.log(b),
                "math::pow" => a.powf(b),
                "math::atan2" => a.atan2(b),
                _ => a.hypot(b),
            }))
        }
        "math::is_nan" | "math::is_finite" | "math::is_infinite" | "math::is_normal" => {
            let n = arg.number()?;
            Ok(Bool(match name {
                "math::is_nan" => n.is_nan(),
                "math::is_finite" => n.is_finite(),
                "math::is_infinite" => n.is_infinite(),
                _ => n.is_normal(),
            }))
        }
        "math::abs" => match arg {
            Int(n) => n.checked_abs().map(Int).ok_or(()),
            Float(n) => Ok(Float(n.abs())),
            _ => Err(()),
        },
        "typeof" => Ok(String(arg.kind().into())),
        "min" | "max" => {
            let minimum = name == "min";
            let mut integer = if minimum { i64::MAX } else { i64::MIN };
            let mut float = if minimum {
                f64::INFINITY
            } else {
                f64::NEG_INFINITY
            };
            for value in arg.tuple()? {
                match value {
                    Int(n) => {
                        integer = if minimum {
                            integer.min(*n)
                        } else {
                            integer.max(*n)
                        }
                    }
                    Float(n) => {
                        float = if minimum {
                            float.min(*n)
                        } else {
                            float.max(*n)
                        }
                    }
                    _ => return Err(()),
                }
            }
            Ok(
                if if minimum {
                    (integer as f64) < float
                } else {
                    (integer as f64) > float
                } {
                    Int(integer)
                } else {
                    Float(float)
                },
            )
        }
        "if" => {
            let [condition, yes, no] = arg.arguments()?;
            Ok(if condition.boolean()? { yes } else { no }.clone())
        }
        "contains" | "contains_any" => {
            let [haystack, needle] = arg.arguments()?;
            let haystack = haystack.tuple()?;
            let needles = if name == "contains" {
                std::slice::from_ref(needle)
            } else {
                needle.tuple()?
            };
            if needles.iter().any(|v| matches!(v, Empty | Tuple(_))) {
                return Err(());
            }
            Ok(Bool(needles.iter().any(|v| haystack.contains(v))))
        }
        "len" => match arg {
            String(s) => Ok(Int(s.len() as i64)),
            Tuple(v) => Ok(Int(v.len() as i64)),
            _ => Err(()),
        },
        "str::to_lowercase" => Ok(String(arg.string()?.to_lowercase())),
        "str::to_uppercase" => Ok(String(arg.string()?.to_uppercase())),
        "str::trim" => Ok(String(arg.string()?.trim().into())),
        "str::from" => Ok(String(arg.text())),
        "str::substring" => {
            let args = arg.tuple()?;
            if !(2..=3).contains(&args.len()) {
                return Err(());
            }
            let text = args[0].string()?;
            let start = usize::try_from(args[1].integer()?).map_err(|_| ())?;
            let end = args
                .get(2)
                .map(|v| v.integer().and_then(|n| usize::try_from(n).map_err(|_| ())))
                .transpose()?
                .unwrap_or(text.len());
            text.get(start..end).map(|s| String(s.into())).ok_or(())
        }
        "bitnot" => Ok(Int(!arg.integer()?)),
        "bitand" | "bitor" | "bitxor" | "shl" | "shr" => {
            let [a, b] = arg.arguments()?;
            let (a, b) = (a.integer()?, b.integer()?);
            Ok(Int(match name {
                "bitand" => a & b,
                "bitor" => a | b,
                "bitxor" => a ^ b,
                "shl" => a.checked_shl(b.try_into().map_err(|_| ())?).ok_or(())?,
                _ => a.checked_shr(b.try_into().map_err(|_| ())?).ok_or(())?,
            }))
        }
        _ => Err(()),
    }
}
