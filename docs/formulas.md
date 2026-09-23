# Formulas

A parameter's `func` is evaluated against the current tracking inputs. Inputs are
floating-point numbers. Names are case-sensitive; `Wave1000` and `PingPong1000`
are generated time variables described in [Configuration](configuration.md).

## Numbers and operators

Whole-number literals use signed 64-bit integer arithmetic. Decimal-point or
exponent literals use floating-point arithmetic. Mixing the two produces a
floating-point result. Thus `5 / 2` is `2`, while `5 / 2.0` is `2.5`.
Hexadecimal (`0x10`), binary (`0b10`), and octal (`0o10`) literals are supported.

From highest to lowest precedence:

| Operation | Syntax |
| --- | --- |
| Grouping and function calls | `(expression)`, `math::sin(expression)` |
| Power | `^` |
| Negation and Boolean NOT | `-`, `!` |
| Multiplication, division, remainder | `*`, `/`, `%` |
| Addition, subtraction | `+`, `-` |
| Comparisons | `==`, `!=`, `<`, `<=`, `>`, `>=` |
| Boolean AND | `&&` |
| Boolean OR | `\|\|` |
| Tuple | `,` |
| Sequence | `;` |

Power returns a float and associates left: `2 ^ 3 ^ 2` is `64`.
Power binds more tightly than negation: `-2 ^ 2` is `-4`.
Integer overflow and integer division by zero are evaluation errors.

Equality preserves types: `1 == 1.0` is false. Ordering comparisons accept mixed
numeric types. Boolean operators require `true` or `false`; numbers are not
implicitly converted to Boolean values.

Strings use double quotes, with `\"` and `\\` escapes. String `+` concatenates;
ordering comparisons use lexical order. Commas create tuples, including function
argument lists. Parentheses preserve nested tuples: `len((1, (2, 3)))` is `2`.
Sequences evaluate every expression and return the last: `1; 2; 3` is `3`.
Parenthesize tuples before a semicolon, for example `(1, 2); 3`.

Both `//` line comments and `/* block comments */` are supported outside strings.
Comments are removed before tokenization, so `1/* comment */2` means `12`.

## Functions

All arguments are evaluated, including both branches of `if` and both operands
of Boolean operators. For example, `if(true, 3, MissingInput)` still fails because
the missing input is evaluated.

| Function | Arguments and result |
| --- | --- |
| `math::ln`, `math::log2`, `math::log10` | One number; natural, base-2, or base-10 logarithm |
| `math::log` | Number, base |
| `math::exp`, `math::exp2` | One number; exponential or power of two |
| `math::pow` | Base, exponent |
| `math::sin`, `math::cos`, `math::tan` | One angle in radians |
| `math::asin`, `math::acos`, `math::atan` | One number; inverse trigonometric function |
| `math::sinh`, `math::cosh`, `math::tanh` | One number; hyperbolic function |
| `math::asinh`, `math::acosh`, `math::atanh` | One number; inverse hyperbolic function |
| `math::atan2` | Y, X; angle in radians |
| `math::sqrt`, `math::cbrt` | One number; square or cube root |
| `math::hypot` | Two numbers; hypotenuse |
| `math::abs` | One number; preserves integer or float type |
| `floor`, `round`, `ceil` | One number; returns a float |
| `math::is_nan`, `math::is_finite`, `math::is_infinite`, `math::is_normal` | One number; returns a Boolean |
| `min`, `max` | Two or more numbers; retains the selected numeric type, preferring float on ties |
| `if` | Boolean condition, true result, false result |
| `typeof` | One value; `"int"`, `"float"`, `"boolean"`, `"string"`, `"tuple"`, or `"empty"` |
| `len` | String byte length or tuple element count |
| `contains` | Tuple, scalar value; equality is type-sensitive |
| `contains_any` | Two tuples; whether they share a scalar value |
| `str::to_lowercase`, `str::to_uppercase`, `str::trim` | One string |
| `str::from` | One value; converts it to text |
| `str::substring` | String, start byte offset, optional exclusive end byte offset |
| `bitand`, `bitor`, `bitxor` | Two integers |
| `bitnot` | One integer |
| `shl`, `shr` | Integer, shift count (0 through 63) |

`math::pi()` is replaced with π. The names `math::min`, `math::max`,
`math::floor`, and `math::ceil` are also accepted. These substitutions retain
the existing behavior of applying throughout the formula text.

## Results and errors

A parameter needs a numeric result. Missing inputs, unknown functions, type
errors, nonnumeric results, and nonfinite results use its `defaultValue`.
Finite results are limited to −1,000,000 through +1,000,000. Empty or
syntactically invalid formulas omit the parameter from expression output.

Tracking inputs are read-only. Assignment syntax remains recognized for saved
formulas, but assignments cannot change inputs and use the error fallback.
There are no loops, user-defined functions, random functions, or regular
expression functions.

Substring offsets must fall on UTF-8 character boundaries. Invalid offsets,
invalid shifts, and overflowing integer absolute values return an evaluation
error instead of panicking.
