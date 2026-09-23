use super::Rule;

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Value {
    Int(i64),
    Float(f64),
    Bool(bool),
    String(String),
    Tuple(Vec<Value>),
    Empty,
}

pub(super) type Result<T> = std::result::Result<T, ()>;

impl Value {
    pub fn number(&self) -> Result<f64> {
        match self {
            Self::Int(n) => Ok(*n as f64),
            Self::Float(n) => Ok(*n),
            _ => Err(()),
        }
    }
    pub fn integer(&self) -> Result<i64> {
        if let Self::Int(n) = self {
            Ok(*n)
        } else {
            Err(())
        }
    }
    pub fn boolean(&self) -> Result<bool> {
        if let Self::Bool(b) = self {
            Ok(*b)
        } else {
            Err(())
        }
    }
    pub fn string(&self) -> Result<&str> {
        if let Self::String(s) = self {
            Ok(s)
        } else {
            Err(())
        }
    }
    pub fn tuple(&self) -> Result<&[Value]> {
        if let Self::Tuple(v) = self {
            Ok(v)
        } else {
            Err(())
        }
    }
    pub fn arguments<const N: usize>(&self) -> Result<&[Value; N]> {
        self.tuple()?.try_into().map_err(|_| ())
    }
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Int(_) => "int",
            Self::Float(_) => "float",
            Self::Bool(_) => "boolean",
            Self::String(_) => "string",
            Self::Tuple(_) => "tuple",
            Self::Empty => "empty",
        }
    }
    pub fn text(&self) -> String {
        match self {
            Self::Int(n) => n.to_string(),
            Self::Float(n) => n.to_string(),
            Self::Bool(b) => b.to_string(),
            Self::String(s) => s.clone(),
            Self::Empty => "()".into(),
            Self::Tuple(v) => format!(
                "({})",
                v.iter()
                    .map(|v| match v {
                        Self::String(s) => format!("\"{s}\""),
                        _ => v.text(),
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
    pub fn unary(self, op: Rule) -> Result<Self> {
        match (op, self) {
            (Rule::negate, Self::Int(n)) => n.checked_neg().map(Self::Int).ok_or(()),
            (Rule::negate, Self::Float(n)) => Ok(Self::Float(-n)),
            (Rule::not, Self::Bool(b)) => Ok(Self::Bool(!b)),
            _ => Err(()),
        }
    }
    pub fn binary(self, op: Rule, right: Self) -> Result<Self> {
        use Rule::*;
        if matches!(op, equal | unequal) {
            return Ok(Self::Bool((self == right) == (op == equal)));
        }
        if matches!(op, and | or) {
            let (a, b) = (self.boolean()?, right.boolean()?);
            return Ok(Self::Bool(if op == and { a && b } else { a || b }));
        }
        if let (Self::String(a), Self::String(b)) = (&self, &right) {
            return match op {
                add => Ok(Self::String(format!("{a}{b}"))),
                less => Ok(Self::Bool(a < b)),
                less_equal => Ok(Self::Bool(a <= b)),
                greater => Ok(Self::Bool(a > b)),
                greater_equal => Ok(Self::Bool(a >= b)),
                _ => Err(()),
            };
        }
        if let (Self::Int(a), Self::Int(b)) = (&self, &right) {
            let result = match op {
                add => a.checked_add(*b),
                subtract => a.checked_sub(*b),
                multiply => a.checked_mul(*b),
                divide => a.checked_div(*b),
                modulo => a.checked_rem(*b),
                less => return Ok(Self::Bool(a < b)),
                less_equal => return Ok(Self::Bool(a <= b)),
                greater => return Ok(Self::Bool(a > b)),
                greater_equal => return Ok(Self::Bool(a >= b)),
                power => return Ok(Self::Float((*a as f64).powf(*b as f64))),
                _ => return Err(()),
            };
            return result.map(Self::Int).ok_or(());
        }
        let (a, b) = (self.number()?, right.number()?);
        Ok(match op {
            add => Self::Float(a + b),
            subtract => Self::Float(a - b),
            multiply => Self::Float(a * b),
            divide => Self::Float(a / b),
            modulo => Self::Float(a % b),
            power => Self::Float(a.powf(b)),
            less => Self::Bool(a < b),
            less_equal => Self::Bool(a <= b),
            greater => Self::Bool(a > b),
            greater_equal => Self::Bool(a >= b),
            _ => return Err(()),
        })
    }
}
