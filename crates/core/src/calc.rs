//! Calculation worksheets (ProseCraft's `src/lib/rich/calc.ts`): a small MathCAD-style
//! evaluator. Each line is one region, evaluated top to bottom: `# text`, `m := 5 kg`,
//! `F := m a =`, `F = kN`, `f(x) := x^2 + 1`. Quantities carry SI dimensions, so mixing units
//! is reported on its line. Numbers are formatted exactly as JavaScript formats them.

use crate::html::is_js_whitespace;

const MAX_LINES: usize = 200;
const MAX_CALLS: usize = 10_000;
const MAX_DEPTH: usize = 64;
const EPSILON: f64 = 1e-9;
/// `MAX_CALC_SOURCE_LENGTH`.
pub const MAX_SOURCE_LENGTH: usize = 8_000;

/// Exponents of m, kg, s, A, K, mol, cd.
type Dims = [f64; 7];

#[derive(Clone, Copy, Debug, PartialEq)]
struct Quantity {
    value: f64,
    dims: Dims,
}

const fn d(l: f64, m: f64, t: f64, i: f64, k: f64, n: f64, j: f64) -> Dims {
    [l, m, t, i, k, n, j]
}

const DIMENSIONLESS: Dims = d(0., 0., 0., 0., 0., 0., 0.);
const BASE_SYMBOLS: [&str; 7] = ["m", "kg", "s", "A", "K", "mol", "cd"];
const LENGTH: Dims = d(1., 0., 0., 0., 0., 0., 0.);
const MASS: Dims = d(0., 1., 0., 0., 0., 0., 0.);
const TIME: Dims = d(0., 0., 1., 0., 0., 0., 0.);
const FORCE: Dims = d(1., 1., -2., 0., 0., 0., 0.);
const ENERGY: Dims = d(2., 1., -2., 0., 0., 0., 0.);
const POWER: Dims = d(2., 1., -3., 0., 0., 0., 0.);
const PRESSURE: Dims = d(-1., 1., -2., 0., 0., 0., 0.);
const CHARGE: Dims = d(0., 0., 1., 1., 0., 0., 0.);
const VOLTAGE: Dims = d(2., 1., -3., -1., 0., 0., 0.);
const RESISTANCE: Dims = d(2., 1., -3., -2., 0., 0., 0.);
const VOLUME: Dims = d(3., 0., 0., 0., 0., 0., 0.);
const FREQUENCY: Dims = d(0., 0., -1., 0., 0., 0., 0.);
const SPEED: Dims = d(1., 0., -1., 0., 0., 0., 0.);
const CURRENT: Dims = d(0., 0., 0., 1., 0., 0., 0.);

/// `UNITS`.
const UNITS: &[(&str, f64, Dims)] = &[
    ("m", 1., LENGTH),
    ("km", 1e3, LENGTH),
    ("cm", 1e-2, LENGTH),
    ("mm", 1e-3, LENGTH),
    ("μm", 1e-6, LENGTH),
    ("nm", 1e-9, LENGTH),
    ("in", 0.0254, LENGTH),
    ("ft", 0.3048, LENGTH),
    ("yd", 0.9144, LENGTH),
    ("mi", 1609.344, LENGTH),
    ("kg", 1., MASS),
    ("gm", 1e-3, MASS),
    ("mg", 1e-6, MASS),
    ("tonne", 1e3, MASS),
    ("lb", 0.45359237, MASS),
    ("s", 1., TIME),
    ("ms", 1e-3, TIME),
    ("μs", 1e-6, TIME),
    ("min", 60., TIME),
    ("hr", 3600., TIME),
    ("day", 86_400., TIME),
    ("yr", 31_557_600., TIME),
    ("A", 1., CURRENT),
    ("mA", 1e-3, CURRENT),
    ("K", 1., d(0., 0., 0., 0., 1., 0., 0.)),
    ("mol", 1., d(0., 0., 0., 0., 0., 1., 0.)),
    ("cd", 1., d(0., 0., 0., 0., 0., 0., 1.)),
    ("N", 1., FORCE),
    ("kN", 1e3, FORCE),
    ("MN", 1e6, FORCE),
    ("lbf", 4.4482216152605, FORCE),
    ("J", 1., ENERGY),
    ("kJ", 1e3, ENERGY),
    ("MJ", 1e6, ENERGY),
    ("kWh", 3.6e6, ENERGY),
    ("cal", 4.184, ENERGY),
    ("kcal", 4184., ENERGY),
    ("eV", 1.602176634e-19, ENERGY),
    ("W", 1., POWER),
    ("kW", 1e3, POWER),
    ("MW", 1e6, POWER),
    ("hp", 745.69987158227, POWER),
    ("Pa", 1., PRESSURE),
    ("kPa", 1e3, PRESSURE),
    ("MPa", 1e6, PRESSURE),
    ("GPa", 1e9, PRESSURE),
    ("bar", 1e5, PRESSURE),
    ("atm", 101_325., PRESSURE),
    ("psi", 6894.757293168, PRESSURE),
    ("coul", 1., CHARGE),
    ("V", 1., VOLTAGE),
    ("mV", 1e-3, VOLTAGE),
    ("kV", 1e3, VOLTAGE),
    ("Ω", 1., RESISTANCE),
    ("ohm", 1., RESISTANCE),
    ("kΩ", 1e3, RESISTANCE),
    ("L", 1e-3, VOLUME),
    ("mL", 1e-6, VOLUME),
    ("gal", 0.003785411784, VOLUME),
    ("Hz", 1., FREQUENCY),
    ("kHz", 1e3, FREQUENCY),
    ("MHz", 1e6, FREQUENCY),
    ("GHz", 1e9, FREQUENCY),
    ("mph", 0.44704, SPEED),
    ("kph", 1. / 3.6, SPEED),
    ("rad", 1., DIMENSIONLESS),
    ("deg", std::f64::consts::PI / 180., DIMENSIONLESS),
    ("°", std::f64::consts::PI / 180., DIMENSIONLESS),
];

/// `UNIT_ALIASES`.
fn alias(name: &str) -> &str {
    match name {
        "um" | "µm" => "μm",
        "us" | "µs" => "μs",
        _ => name,
    }
}

fn unit_named(name: &str) -> Option<Quantity> {
    let name = alias(name);
    UNITS
        .iter()
        .find(|(n, ..)| *n == name)
        .map(|&(_, value, dims)| Quantity { value, dims })
}

/// `CONSTANTS`.
fn constant(name: &str) -> Option<Quantity> {
    let value = match name {
        "π" | "pi" => std::f64::consts::PI,
        "e" => std::f64::consts::E,
        "g" => {
            return Some(Quantity {
                value: 9.80665,
                dims: d(1., 0., -2., 0., 0., 0., 0.),
            });
        }
        _ => return None,
    };
    Some(Quantity {
        value,
        dims: DIMENSIONLESS,
    })
}

/// `DISPLAY_UNITS`.
const DISPLAY_UNITS: [(&str, Dims); 6] = [
    ("N", FORCE),
    ("J", ENERGY),
    ("W", POWER),
    ("Pa", PRESSURE),
    ("V", VOLTAGE),
    ("Ω", RESISTANCE),
];

const GREEK: [&str; 32] = [
    "alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta", "iota", "kappa",
    "lambda", "mu", "nu", "xi", "rho", "sigma", "tau", "upsilon", "phi", "chi", "psi", "omega",
    "Gamma", "Delta", "Theta", "Lambda", "Xi", "Sigma", "Upsilon", "Phi", "Psi", "Omega",
];

/// `CalcError`: what is wrong with a line, as ProseCraft says it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CalcError(pub String);

type Result<T> = std::result::Result<T, CalcError>;

fn fail<T>(message: impl Into<String>) -> Result<T> {
    Err(CalcError(message.into()))
}

// ---------------------------------------------------------------------------------------------
// Tokens and syntax tree

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Number(String, f64),
    Ident(String),
    Op(char),
}

impl Token {
    fn text(&self) -> String {
        match self {
            Self::Number(t, _) | Self::Ident(t) => t.clone(),
            Self::Op(c) => c.to_string(),
        }
    }
}

/// A unit chain: the unit names written straight after a number.
#[derive(Clone, Debug, Default)]
struct Chain {
    names: Vec<String>,
    ended_by_operand: bool,
}

#[derive(Clone, Debug)]
enum Node {
    Number(String, f64),
    Ident {
        name: String,
        unit: bool,
        chain: Option<usize>,
    },
    Neg(Box<Node>),
    Binary {
        op: char,
        left: Box<Node>,
        right: Box<Node>,
        implicit: bool,
    },
    Call(String, Vec<Node>),
}

/// `IDENT_START`: ASCII letters, `_`, the Greek and Coptic block, `µ`, `°`.
fn ident_start(c: char) -> bool {
    c.is_ascii_alphabetic()
        || c == '_'
        || ('\u{370}'..='\u{3ff}').contains(&c)
        || c == 'µ'
        || c == '°'
}

/// `IDENT_PART`.
fn ident_part(c: char) -> bool {
    c.is_ascii_alphanumeric()
        || c == '_'
        || ('\u{370}'..='\u{3ff}').contains(&c)
        || c == 'µ'
        || c == '\''
}

/// `^(?:\d+\.?\d*|\.\d+)(?:[eE][+-]?\d+)?`: the length of a number at the start.
fn number_length(chars: &[char]) -> usize {
    let digits = |from: usize| {
        chars[from..]
            .iter()
            .take_while(|c| c.is_ascii_digit())
            .count()
    };
    let mut end;
    let lead = digits(0);
    if lead > 0 {
        end = lead;
        if chars.get(end) == Some(&'.') {
            end += 1;
            end += digits(end);
        }
    } else if chars.first() == Some(&'.') && digits(1) > 0 {
        end = 1 + digits(1);
    } else {
        return 0;
    }
    if matches!(chars.get(end), Some('e' | 'E')) {
        let mut at = end + 1;
        if matches!(chars.get(at), Some('+' | '-')) {
            at += 1;
        }
        let exponent = digits(at);
        if exponent > 0 {
            end = at + exponent;
        }
    }
    end
}

fn tokenize(text: &str) -> Result<Vec<Token>> {
    let chars: Vec<char> = text.chars().collect();
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        let c = chars[index];
        if is_js_whitespace(c) {
            index += 1;
            continue;
        }
        let length = number_length(&chars[index..]);
        if length > 0 {
            let text: String = chars[index..index + length].iter().collect();
            let value = text.parse::<f64>().unwrap_or(f64::NAN);
            tokens.push(Token::Number(text, value));
            index += length;
            continue;
        }
        if ident_start(c) {
            // The degree sign is a complete unit on its own, so "30°" works.
            let mut end = index + 1;
            if c != '°' {
                while end < chars.len() && ident_part(chars[end]) {
                    end += 1;
                }
            }
            tokens.push(Token::Ident(chars[index..end].iter().collect()));
            index = end;
            continue;
        }
        let op = match c {
            '·' | '×' => '*',
            '−' => '-',
            c => c,
        };
        if "+-*/^(),".contains(op) {
            tokens.push(Token::Op(op));
            index += 1;
            continue;
        }
        return fail(format!("Unexpected character “{c}”."));
    }
    Ok(tokens)
}

struct Parser {
    tokens: Vec<Token>,
    index: usize,
    chains: Vec<Chain>,
}

/// `unitTarget`: the identifier a multiplicative operand names, looking through a power.
fn unit_target(node: &mut Node) -> Option<&mut Node> {
    match node {
        Node::Ident { .. } => Some(node),
        Node::Binary { op: '^', left, .. } if matches!(**left, Node::Ident { .. }) => Some(left),
        _ => None,
    }
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            index: 0,
            chains: Vec::new(),
        }
    }

    fn parse(&mut self) -> Result<Node> {
        if self.tokens.is_empty() {
            return fail("Missing expression.");
        }
        let node = self.additive()?;
        if let Some(extra) = self.peek() {
            return fail(format!("Unexpected “{}”.", extra.text()));
        }
        Ok(node)
    }

    /// `parseUnits`: every unit name means the unit, even if it was redefined.
    fn parse_units(&mut self) -> Result<Node> {
        fn mark(node: &mut Node) {
            match node {
                Node::Ident { name, unit, .. } => *unit = unit_named(name).is_some(),
                Node::Neg(operand) => mark(operand),
                Node::Binary { left, right, .. } => {
                    mark(left);
                    mark(right);
                }
                _ => {}
            }
        }
        let mut node = self.parse()?;
        mark(&mut node);
        Ok(node)
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.index)
    }

    /// The operator at the cursor, taken (`this.tokens[this.index++].text`).
    fn take_op(&mut self) -> char {
        let op = match self.peek() {
            Some(Token::Op(op)) => *op,
            _ => ' ',
        };
        self.index += 1;
        op
    }

    fn is_op(&self, op: char) -> bool {
        self.peek() == Some(&Token::Op(op))
    }

    fn expect_op(&mut self, op: char) -> Result<()> {
        if !self.is_op(op) {
            return fail(format!("Expected “{op}”."));
        }
        self.index += 1;
        Ok(())
    }

    fn additive(&mut self) -> Result<Node> {
        let mut node = self.multiplicative()?;
        while self.is_op('+') || self.is_op('-') {
            let op = self.take_op();
            let right = self.multiplicative()?;
            node = Node::Binary {
                op,
                left: Box::new(node),
                right: Box::new(right),
                implicit: false,
            };
        }
        Ok(node)
    }

    /// Juxtaposition multiplies; unit names straight after a number form a unit chain.
    fn multiplicative(&mut self) -> Result<Node> {
        let mut node = self.unary()?;
        let mut chain: Option<usize> = None;
        loop {
            let (op, right, implicit) = if self.is_op('*') || self.is_op('/') {
                let op = self.take_op();
                (op, self.unary()?, false)
            } else {
                match self.peek() {
                    None => return Ok(node),
                    Some(Token::Op(c)) if *c != '(' => return Ok(node),
                    _ => {}
                }
                let starts_chain = matches!(node, Node::Number(..));
                let right = self.power()?;
                if starts_chain {
                    self.chains.push(Chain::default());
                    chain = Some(self.chains.len() - 1);
                }
                ('*', right, true)
            };
            let mut right = right;
            let linked = match (chain, unit_target(&mut right)) {
                (
                    Some(c),
                    Some(Node::Ident {
                        name,
                        unit,
                        chain: link,
                    }),
                ) if unit_named(name).is_some() => {
                    *unit = true;
                    *link = Some(c);
                    self.chains[c].names.push(name.clone());
                    true
                }
                _ => false,
            };
            if !linked && let Some(c) = chain.take() {
                self.chains[c].ended_by_operand = !self.chains[c].names.is_empty();
            }
            node = Node::Binary {
                op,
                left: Box::new(node),
                right: Box::new(right),
                implicit,
            };
        }
    }

    fn unary(&mut self) -> Result<Node> {
        if self.is_op('-') {
            self.index += 1;
            return Ok(Node::Neg(Box::new(self.unary()?)));
        }
        if self.is_op('+') {
            self.index += 1;
            return self.unary();
        }
        self.power()
    }

    fn power(&mut self) -> Result<Node> {
        let base = self.primary()?;
        if !self.is_op('^') {
            return Ok(base);
        }
        self.index += 1;
        Ok(Node::Binary {
            op: '^',
            left: Box::new(base),
            right: Box::new(self.unary()?),
            implicit: false,
        })
    }

    fn primary(&mut self) -> Result<Node> {
        let Some(token) = self.peek().cloned() else {
            return fail("Expression ends too early.");
        };
        self.index += 1;
        match token {
            Token::Number(text, value) => Ok(Node::Number(text, value)),
            Token::Ident(name) => {
                if !self.is_op('(') {
                    return Ok(Node::Ident {
                        name,
                        unit: false,
                        chain: None,
                    });
                }
                self.index += 1;
                let mut args = Vec::new();
                if !self.is_op(')') {
                    args.push(self.additive()?);
                    while self.is_op(',') {
                        self.index += 1;
                        args.push(self.additive()?);
                    }
                }
                self.expect_op(')')?;
                Ok(Node::Call(name, args))
            }
            Token::Op('(') => {
                let node = self.additive()?;
                self.expect_op(')')?;
                Ok(node)
            }
            other => fail(format!("Unexpected “{}”.", other.text())),
        }
    }
}

/// An expression and its unit chains.
struct Parsed {
    node: Node,
    chains: Vec<Chain>,
}

fn parse_expression(text: &str) -> Result<Parsed> {
    let mut parser = Parser::new(tokenize(text)?);
    let node = parser.parse()?;
    Ok(Parsed {
        node,
        chains: parser.chains,
    })
}

fn parse_units(text: &str) -> Result<Parsed> {
    let mut parser = Parser::new(tokenize(text)?);
    let node = parser.parse_units()?;
    Ok(Parsed {
        node,
        chains: parser.chains,
    })
}

// ---------------------------------------------------------------------------------------------
// Evaluation

#[derive(Clone)]
struct UserFunction {
    params: Vec<String>,
    body: Parsed,
}

impl Clone for Parsed {
    fn clone(&self) -> Self {
        Self {
            node: self.node.clone(),
            chains: self.chains.clone(),
        }
    }
}

/// Insertion-ordered maps are not needed: names are looked up only.
#[derive(Default)]
struct Scope {
    variables: std::collections::HashMap<String, Quantity>,
    functions: std::collections::HashMap<String, UserFunction>,
    calls: usize,
}

type Locals = std::collections::HashMap<String, Quantity>;

fn same_dims(a: &Dims, b: &Dims) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < EPSILON)
}

fn is_dimensionless(q: &Quantity) -> bool {
    same_dims(&q.dims, &DIMENSIONLESS)
}

fn require_dimensionless(q: &Quantity, what: &str) -> Result<f64> {
    if !is_dimensionless(q) {
        return fail(format!("{what} must be unitless."));
    }
    Ok(q.value)
}

fn finite(q: Quantity) -> Result<Quantity> {
    if !q.value.is_finite() {
        return fail("The result is undefined or infinite.");
    }
    Ok(q)
}

fn map_dims(dims: &Dims, f: impl Fn(f64) -> f64) -> Dims {
    dims.map(f)
}

/// `Math.round`: halves toward +∞.
fn js_round(x: f64) -> f64 {
    let floor = x.floor();
    if x - floor >= 0.5 { floor + 1.0 } else { floor }
}

const BUILTINS: [&str; 16] = [
    "sqrt", "abs", "sin", "cos", "tan", "asin", "acos", "atan", "ln", "log", "exp", "floor",
    "ceil", "round", "min", "max",
];

fn is_builtin(name: &str) -> bool {
    BUILTINS.contains(&name)
}

fn extremum(args: &[Quantity], max: bool) -> Result<Quantity> {
    let first = args[0];
    if args.iter().any(|a| !same_dims(&a.dims, &first.dims)) {
        return fail("Every value must have the same units.");
    }
    // `Math.min` / `Math.max`: NaN wins; −0 is less than +0.
    let mut value = if max {
        f64::NEG_INFINITY
    } else {
        f64::INFINITY
    };
    for a in args {
        let v = a.value;
        if v.is_nan() || value.is_nan() {
            value = f64::NAN;
        } else if (max
            && (v > value || (v == value && value.is_sign_negative() && v.is_sign_positive())))
            || (!max && (v < value || (v == value && v.is_sign_negative())))
        {
            value = v;
        }
    }
    Ok(Quantity {
        value,
        dims: first.dims,
    })
}

fn builtin(name: &str, args: &[Quantity]) -> Result<Quantity> {
    let x = args.first().copied().unwrap_or(Quantity {
        value: f64::NAN,
        dims: DIMENSIONLESS,
    });
    let plain = |value: f64| Quantity {
        value,
        dims: DIMENSIONLESS,
    };
    Ok(match name {
        "sqrt" => Quantity {
            value: x.value.sqrt(),
            dims: map_dims(&x.dims, |d| d / 2.0),
        },
        "abs" => Quantity {
            value: x.value.abs(),
            dims: x.dims,
        },
        "sin" => plain(require_dimensionless(&x, "An angle")?.sin()),
        "cos" => plain(require_dimensionless(&x, "An angle")?.cos()),
        "tan" => plain(require_dimensionless(&x, "An angle")?.tan()),
        "asin" => plain(require_dimensionless(&x, "The argument")?.asin()),
        "acos" => plain(require_dimensionless(&x, "The argument")?.acos()),
        "atan" => plain(require_dimensionless(&x, "The argument")?.atan()),
        "ln" => plain(require_dimensionless(&x, "A logarithm's argument")?.ln()),
        "log" => plain(require_dimensionless(&x, "A logarithm's argument")?.log10()),
        "exp" => plain(require_dimensionless(&x, "An exponent")?.exp()),
        "floor" => Quantity {
            value: x.value.floor(),
            dims: x.dims,
        },
        "ceil" => Quantity {
            value: x.value.ceil(),
            dims: x.dims,
        },
        "round" => Quantity {
            value: js_round(x.value),
            dims: x.dims,
        },
        "min" => extremum(args, false)?,
        _ => extremum(args, true)?,
    })
}

/// `readsAsUnit`: whether a unit-named identifier means the unit.
fn reads_as_unit(
    name: &str,
    unit: bool,
    chain: Option<usize>,
    chains: &[Chain],
    is_variable: &dyn Fn(&str) -> bool,
) -> Result<bool> {
    if !unit {
        return Ok(false);
    }
    let Some(chain) = chain.and_then(|c| chains.get(c)) else {
        return Ok(true);
    };
    if !is_variable(name) {
        return Ok(true);
    }
    if chain.names.iter().any(|n| !is_variable(n)) {
        return Ok(true);
    }
    if chain.ended_by_operand {
        return Ok(false);
    }
    fail(format!(
        "{name} is both a unit and a variable here. Write *{name} to use the variable, or rename it."
    ))
}

fn lookup(name: &str, scope: &Scope, locals: &Locals) -> Option<Quantity> {
    locals
        .get(name)
        .or_else(|| scope.variables.get(name))
        .copied()
        .or_else(|| constant(name))
        .or_else(|| unit_named(name))
}

fn multiply(left: Quantity, right: Quantity) -> Result<Quantity> {
    let mut dims = left.dims;
    for (d, r) in dims.iter_mut().zip(right.dims) {
        *d += r;
    }
    finite(Quantity {
        value: left.value * right.value,
        dims,
    })
}

/// `valueBeforeParentheses`: "x (y + 1)" parses as a call; a value there multiplies.
fn value_before_parentheses(node: &Node, scope: &Scope, locals: &Locals) -> Option<Quantity> {
    let Node::Call(name, args) = node else {
        return None;
    };
    if args.len() != 1 || scope.functions.contains_key(name) || is_builtin(name) {
        return None;
    }
    lookup(name, scope, locals)
}

fn evaluate(
    node: &Node,
    chains: &[Chain],
    scope: &mut Scope,
    locals: &Locals,
    depth: usize,
) -> Result<Quantity> {
    match node {
        Node::Number(_, value) => Ok(Quantity {
            value: *value,
            dims: DIMENSIONLESS,
        }),
        Node::Ident { name, unit, chain } => {
            let is_variable = |n: &str| locals.contains_key(n) || scope.variables.contains_key(n);
            if reads_as_unit(name, *unit, *chain, chains, &is_variable)? {
                return unit_named(name)
                    .ok_or_else(|| CalcError(format!("{name} is not defined.")));
            }
            if let Some(value) = lookup(name, scope, locals) {
                return Ok(value);
            }
            if scope.functions.contains_key(name) || is_builtin(name) {
                return fail(format!("{name} is a function; call it like {name}(x)."));
            }
            fail(format!("{name} is not defined."))
        }
        Node::Neg(operand) => {
            let q = evaluate(operand, chains, scope, locals, depth)?;
            Ok(Quantity {
                value: -q.value,
                dims: q.dims,
            })
        }
        Node::Binary {
            op, left, right, ..
        } => {
            if *op == '^'
                && let Some(value) = value_before_parentheses(left, scope, locals)
                && let Node::Call(_, args) = &**left
            {
                // "x (y)^2" is x·y², not (x·y)².
                let power = Node::Binary {
                    op: '^',
                    left: Box::new(args[0].clone()),
                    right: right.clone(),
                    implicit: false,
                };
                let squared = evaluate(&power, chains, scope, locals, depth)?;
                return multiply(value, squared);
            }
            let l = evaluate(left, chains, scope, locals, depth)?;
            let r = evaluate(right, chains, scope, locals, depth)?;
            match op {
                '+' | '-' => {
                    if !same_dims(&l.dims, &r.dims) {
                        return fail(format!(
                            "The units don't match: {} and {}.",
                            plain_unit(&l.dims),
                            plain_unit(&r.dims)
                        ));
                    }
                    finite(Quantity {
                        value: if *op == '+' {
                            l.value + r.value
                        } else {
                            l.value - r.value
                        },
                        dims: l.dims,
                    })
                }
                '*' => multiply(l, r),
                '/' => {
                    if r.value == 0.0 {
                        return fail("Division by zero.");
                    }
                    let mut dims = l.dims;
                    for (d, x) in dims.iter_mut().zip(r.dims) {
                        *d -= x;
                    }
                    finite(Quantity {
                        value: l.value / r.value,
                        dims,
                    })
                }
                _ => {
                    let exponent = require_dimensionless(&r, "An exponent")?;
                    finite(Quantity {
                        value: js_pow(l.value, exponent),
                        dims: map_dims(&l.dims, |d| d * exponent),
                    })
                }
            }
        }
        Node::Call(name, args) => {
            scope.calls += 1;
            if depth > MAX_DEPTH || scope.calls > MAX_CALLS {
                return fail(
                    "The calculation is too deep. Check for a function that calls itself.",
                );
            }
            let mut values = Vec::with_capacity(args.len());
            for arg in args {
                values.push(evaluate(arg, chains, scope, locals, depth + 1)?);
            }
            if let Some(user) = scope.functions.get(name).cloned() {
                if values.len() != user.params.len() {
                    let n = user.params.len();
                    return fail(format!(
                        "{name} takes {n} argument{}.",
                        if n == 1 { "" } else { "s" }
                    ));
                }
                let inner: Locals = user.params.iter().cloned().zip(values).collect();
                return evaluate(&user.body.node, &user.body.chains, scope, &inner, depth + 1);
            }
            if !is_builtin(name) {
                if let Some(value) = value_before_parentheses(node, scope, locals) {
                    return multiply(value, values[0]);
                }
                return fail(format!("{name} is not a known function."));
            }
            let variadic = name == "min" || name == "max";
            if (!variadic && values.len() != 1) || (variadic && values.is_empty()) {
                return fail(format!(
                    "{name} takes {}.",
                    if variadic {
                        "at least one argument"
                    } else {
                        "one argument"
                    }
                ));
            }
            finite(builtin(name, &values)?)
        }
    }
}

/// `**`: as JavaScript's (a NaN exponent is NaN; 1 and −1 to ±∞ are NaN).
fn js_pow(base: f64, exponent: f64) -> f64 {
    if exponent.is_nan() || (base.abs() == 1.0 && exponent.is_infinite()) {
        return f64::NAN;
    }
    base.powf(exponent)
}

// ---------------------------------------------------------------------------------------------
// Number formatting, as JavaScript's `toFixed`, `toPrecision` and `toExponential`

/// The magnitude's decimal digits (exact to 80 places) and the power of ten of the first.
fn exact_digits(x: f64) -> (Vec<u8>, i32) {
    let text = format!("{:.80e}", x.abs());
    let (mantissa, exponent) = text.split_once('e').unwrap_or((&text, "0"));
    let digits = mantissa
        .bytes()
        .filter(u8::is_ascii_digit)
        .map(|b| b - b'0')
        .collect();
    (digits, exponent.parse().unwrap_or(0))
}

/// The magnitude rounded half up to `count` significant digits: the digits and the power of
/// ten of the first (a carry can raise it).
fn round_significant(x: f64, count: usize) -> (Vec<u8>, i32) {
    let (digits, mut exponent) = exact_digits(x);
    let mut kept: Vec<u8> = digits.iter().take(count).copied().collect();
    kept.resize(count, 0);
    if digits.get(count).is_some_and(|d| *d >= 5) {
        let mut i = count;
        loop {
            if i == 0 {
                kept.insert(0, 1);
                kept.truncate(count);
                exponent += 1;
                break;
            }
            i -= 1;
            if kept[i] == 9 {
                kept[i] = 0;
            } else {
                kept[i] += 1;
                break;
            }
        }
    }
    (kept, exponent)
}

fn sign(x: f64) -> &'static str {
    if x < 0.0 { "-" } else { "" }
}

fn digits_text(digits: &[u8]) -> String {
    digits.iter().map(|d| char::from(b'0' + d)).collect()
}

/// `toFixed(places)` (for magnitudes below 10²¹).
fn to_fixed(x: f64, places: usize) -> String {
    if x == 0.0 {
        return format!(
            "{}{}",
            if places > 0 { "0." } else { "0" },
            "0".repeat(places)
        );
    }
    let (_, exponent) = exact_digits(x);
    let wanted = exponent + 1 + places as i32;
    let (digits, exponent) = if wanted <= 0 {
        // Rounds to zero, or up to the last place.
        let (all, e) = exact_digits(x);
        let up = wanted == 0 && all.first().is_some_and(|d| *d >= 5);
        if up {
            (vec![1], e + 1)
        } else {
            (Vec::new(), 0)
        }
    } else {
        round_significant(x, wanted as usize)
    };
    // Place the digits: `exponent` is the power of ten of the first.
    let mut int_part = String::new();
    let mut frac = vec![b'0'; places];
    for (i, digit) in digits.iter().enumerate() {
        let power = exponent - i as i32;
        if power >= 0 {
            int_part.push(char::from(b'0' + digit));
        } else {
            let place = (-power - 1) as usize;
            if place < places {
                frac[place] = b'0' + digit;
            }
        }
    }
    if exponent >= 0 && digits.len() < (exponent + 1) as usize {
        int_part.push_str(&"0".repeat((exponent + 1) as usize - digits.len()));
    }
    if int_part.is_empty() {
        int_part.push('0');
    }
    let text = if places > 0 {
        format!("{int_part}.{}", String::from_utf8(frac).unwrap_or_default())
    } else {
        int_part
    };
    format!("{}{text}", sign(x))
}

/// `toPrecision(4)` for magnitudes from 10⁻⁴ to below 1, and `toExponential(3)`.
fn to_precision_4(x: f64) -> String {
    let (digits, exponent) = round_significant(x, 4);
    let text = digits_text(&digits);
    let body = if exponent < 0 {
        format!("0.{}{text}", "0".repeat((-exponent - 1) as usize))
    } else {
        let split = (exponent + 1) as usize;
        if split >= 4 {
            text
        } else {
            format!("{}.{}", &text[..split], &text[split..])
        }
    };
    format!("{}{body}", sign(x))
}

fn to_exponential_3(x: f64) -> (String, i32) {
    let (digits, exponent) = round_significant(x, 4);
    let text = digits_text(&digits);
    (
        format!("{}{}.{}", sign(x), &text[..1], &text[1..]),
        exponent,
    )
}

/// `formatNumber`: the mantissa, and an exponent of ten for very large or small values.
fn format_number(value: f64) -> (String, Option<String>) {
    // A literal like `1e400` is stored as Infinity (literals aren't checked), and JavaScript
    // formats it as "Infinity" with the exponent `Number(undefined)`: ProseCraft shows
    // "Infinity·10^NaN".
    if value.is_infinite() {
        return (format!("{}Infinity", sign(value)), Some("NaN".into()));
    }
    if value.is_nan() {
        return ("NaN".into(), None);
    }
    let magnitude = value.abs();
    if magnitude != 0.0 && (magnitude >= 1e9 || magnitude < 1e-4) {
        let (mantissa, exponent) = to_exponential_3(value);
        return (trim_zeros(&mantissa), Some(exponent.to_string()));
    }
    let text = if magnitude >= 1.0 {
        to_fixed(value, 3)
    } else {
        to_precision_4(value)
    };
    (trim_zeros(&text), None)
}

fn trim_zeros(text: &str) -> String {
    let trimmed = if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.')
    } else {
        text
    };
    if trimmed == "-0" {
        "0".into()
    } else {
        trimmed.into()
    }
}

fn exponent_text(value: f64) -> String {
    trim_zeros(&to_fixed(value, 3))
}

fn unit_parts(dims: &Dims) -> Vec<(&'static str, f64)> {
    if let Some((symbol, _)) = DISPLAY_UNITS.iter().find(|(_, u)| same_dims(u, dims)) {
        return vec![(symbol, 1.0)];
    }
    dims.iter()
        .zip(BASE_SYMBOLS)
        .filter(|(p, _)| p.abs() > EPSILON)
        .map(|(p, s)| (s, *p))
        .collect()
}

fn plain_unit(dims: &Dims) -> String {
    let parts = unit_parts(dims);
    if parts.is_empty() {
        return "no units".into();
    }
    parts
        .iter()
        .map(|(s, p)| {
            if *p == 1.0 {
                (*s).to_owned()
            } else {
                format!("{s}^{}", exponent_text(*p))
            }
        })
        .collect::<Vec<_>>()
        .join("·")
}

fn latex_unit_symbol(symbol: &str) -> String {
    format!(
        "\\mathrm{{{}}}",
        symbol.replace('μ', "\\mu ").replace('Ω', "\\Omega ")
    )
}

fn latex_unit_part(symbol: &str, power: f64) -> String {
    let s = latex_unit_symbol(symbol);
    if power == 1.0 {
        s
    } else {
        format!("{s}^{{{}}}", exponent_text(power))
    }
}

fn latex_dims(dims: &Dims) -> String {
    let parts = unit_parts(dims);
    let numerator: Vec<String> = parts
        .iter()
        .filter(|(_, p)| *p > 0.0)
        .map(|(s, p)| latex_unit_part(s, *p))
        .collect();
    let denominator: Vec<String> = parts
        .iter()
        .filter(|(_, p)| *p < 0.0)
        .map(|(s, p)| latex_unit_part(s, -p))
        .collect();
    if denominator.is_empty() {
        return numerator.join("\\cdot ");
    }
    let top = if numerator.is_empty() {
        "1".to_owned()
    } else {
        numerator.join("\\cdot ")
    };
    format!("\\frac{{{top}}}{{{}}}", denominator.join("\\cdot "))
}

fn plain_number(value: f64) -> String {
    match format_number(value) {
        (m, None) => m,
        (m, Some(e)) => format!("{m}·10^{e}"),
    }
}

fn latex_number(value: f64) -> String {
    match format_number(value) {
        (m, None) => m,
        (m, Some(e)) => format!("{m}\\cdot 10^{{{e}}}"),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Variable,
    Unit,
    Constant,
    Function,
}

fn is_greek_block(text: &str) -> bool {
    let core = text.trim_end_matches('\'');
    !core.is_empty()
        && core.chars().all(|c| ('\u{370}'..='\u{3ff}').contains(&c))
        && text.len() >= core.len()
}

fn latex_identifier(name: &str, kind: Kind) -> String {
    if kind == Kind::Unit {
        if name == "°" {
            return "^{\\circ}".into();
        }
        return latex_unit_symbol(alias(name));
    }
    if name == "pi" || name == "π" {
        return "\\pi ".into();
    }
    let mut parts = name.split('_');
    let base = parts.next().unwrap_or_default();
    let subscript = parts.collect::<Vec<_>>().join("_");
    let head = if GREEK.contains(&base) {
        format!("\\{base} ")
    } else if kind == Kind::Function {
        format!("\\operatorname{{{base}}}")
    } else if base.chars().count() > 1 && !is_greek_block(base) {
        format!("\\mathit{{{base}}}")
    } else {
        base.to_owned()
    };
    if subscript.is_empty() {
        return head;
    }
    format!(
        "{{{head}}}_{{\\mathrm{{{}}}}}",
        subscript.replace('_', "\\_")
    )
}

fn precedence(node: &Node) -> u8 {
    match node {
        Node::Binary { op: '+' | '-', .. } => 1,
        Node::Binary { op: '*' | '/', .. } => 2,
        Node::Binary { op: '^', .. } => 4,
        Node::Binary { .. } => 0,
        Node::Neg(_) => 3,
        Node::Number(..) | Node::Ident { .. } | Node::Call(..) => 5,
    }
}

/// `toLatex`.
fn to_latex(node: &Node, chains: &[Chain], classify: &dyn Fn(&str) -> Kind) -> String {
    let wrap = |child: &Node, minimum: u8| {
        let latex = to_latex(child, chains, classify);
        if precedence(child) < minimum {
            format!("\\left({latex}\\right)")
        } else {
            latex
        }
    };
    match node {
        Node::Number(text, _) => {
            // `/[eE]([+-]?\d+)$/` → `\cdot 10^{n}`.
            match text.find(['e', 'E']) {
                Some(at) => {
                    let exponent: i64 = text[at + 1..].parse().unwrap_or(0);
                    format!("{}\\cdot 10^{{{exponent}}}", &text[..at])
                }
                None => text.clone(),
            }
        }
        Node::Ident { name, unit, chain } => {
            let is_variable = |n: &str| classify(n) == Kind::Variable;
            let as_unit = reads_as_unit(name, *unit, *chain, chains, &is_variable).unwrap_or(true);
            latex_identifier(name, if as_unit { Kind::Unit } else { classify(name) })
        }
        Node::Neg(operand) => format!("-{}", wrap(operand, 3)),
        Node::Call(name, args) => {
            let args: Vec<String> = args.iter().map(|a| to_latex(a, chains, classify)).collect();
            if name == "sqrt" && args.len() == 1 && classify(name) != Kind::Function {
                return format!("\\sqrt{{{}}}", args[0]);
            }
            if name == "abs" && args.len() == 1 && classify(name) != Kind::Function {
                return format!("\\left|{}\\right|", args[0]);
            }
            let kind = classify(name);
            let joined = args.join(", ");
            if kind != Kind::Function && !is_builtin(name) {
                return format!("{}\\left({joined}\\right)", latex_identifier(name, kind));
            }
            format!(
                "{}\\left({joined}\\right)",
                latex_identifier(name, Kind::Function)
            )
        }
        Node::Binary {
            op,
            left,
            right,
            implicit,
        } => match op {
            '+' => format!("{} + {}", wrap(left, 1), wrap(right, 1)),
            '-' => format!("{} - {}", wrap(left, 1), wrap(right, 2)),
            '*' => {
                let degree = matches!(&**right, Node::Ident { name, unit, .. }
                    if name == "°" && (*unit || classify("°") == Kind::Unit));
                let glue = if degree {
                    ""
                } else if *implicit {
                    "\\,"
                } else {
                    "\\cdot "
                };
                format!("{}{glue}{}", wrap(left, 2), wrap(right, 3))
            }
            '/' => format!(
                "\\frac{{{}}}{{{}}}",
                to_latex(left, chains, classify),
                to_latex(right, chains, classify)
            ),
            _ => format!(
                "{{{}}}^{{{}}}",
                wrap(left, 5),
                to_latex(right, chains, classify)
            ),
        },
    }
}

fn escape_text(text: &str) -> String {
    let mut out = String::new();
    for c in text.replace('\\', "/").chars() {
        match c {
            '{' | '}' | '$' | '%' | '#' | '&' | '_' => {
                out.push('\\');
                out.push(c);
            }
            // MathJax text mode has no escape for these, so use look-alikes.
            '^' => out.push('ˆ'),
            '~' => out.push('˜'),
            c => out.push(c),
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Worksheet lines

/// `CalcLine`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CalcLine {
    Blank,
    Text(String),
    Math {
        source: String,
        latex: String,
        plain: String,
    },
    Error {
        source: String,
        message: String,
    },
}

/// `splitEvaluation`.
fn split_evaluation(text: &str) -> Result<(String, bool, String)> {
    let Some(index) = text.find('=') else {
        return Ok((text.to_owned(), false, String::new()));
    };
    if text[index + 1..].contains('=') {
        return fail("Use one “=” per line.");
    }
    Ok((
        text[..index].to_owned(),
        true,
        text[index + 1..].trim_matches(is_js_whitespace).to_owned(),
    ))
}

fn format_result(
    result: Quantity,
    unit_text: &str,
    scope: &mut Scope,
    classify: &dyn Fn(&str) -> Kind,
) -> Result<(String, String)> {
    if unit_text.is_empty() {
        let (latex_unit, plain_unit_text) = if is_dimensionless(&result) {
            (String::new(), String::new())
        } else {
            (
                format!("\\,{}", latex_dims(&result.dims)),
                format!(" {}", plain_unit(&result.dims)),
            )
        };
        return Ok((
            format!("{}{latex_unit}", latex_number(result.value)),
            format!("{}{plain_unit_text}", plain_number(result.value)),
        ));
    }
    let unit_node = parse_units(unit_text)?;
    let unit = evaluate(&unit_node.node, &unit_node.chains, scope, &Locals::new(), 0)?;
    if !same_dims(&unit.dims, &result.dims) {
        return fail(format!(
            "The result is in {}, which can't be shown in {unit_text}.",
            plain_unit(&result.dims)
        ));
    }
    if unit.value == 0.0 {
        return fail(format!("{unit_text} can't be used as a display unit."));
    }
    let value = result.value / unit.value;
    let unit_latex = to_latex(&unit_node.node, &unit_node.chains, classify);
    let degree = unit_text == "°";
    Ok((
        format!(
            "{}{}{unit_latex}",
            latex_number(value),
            if degree { "" } else { "\\," }
        ),
        format!(
            "{}{}{unit_text}",
            plain_number(value),
            if degree { "" } else { " " }
        ),
    ))
}

fn is_identifier(text: &str) -> bool {
    let mut chars = text.chars();
    chars.next().is_some_and(ident_start) && chars.all(ident_part)
}

/// `DEFINITION`: `name(params) := rest`.
fn definition(line: &str) -> Option<(String, Option<String>, String)> {
    let at = line.find(":=")?;
    let head = &line[..at];
    let rest = line[at + 2..].to_owned();
    // `^\s*([^\s(:=]+)\s*(?:\(([^)]*)\))?\s*` must match all of `head`.
    let head = head.trim_start_matches(is_js_whitespace);
    let name_end = head
        .find(|c: char| is_js_whitespace(c) || c == '(' || c == ':' || c == '=')
        .unwrap_or(head.len());
    if name_end == 0 {
        return None;
    }
    let name = head[..name_end].to_owned();
    let after = head[name_end..].trim_start_matches(is_js_whitespace);
    let (params, after) = if let Some(inner) = after.strip_prefix('(') {
        let close = inner.find(')')?;
        (Some(inner[..close].to_owned()), &inner[close + 1..])
    } else {
        (None, after)
    };
    if !after.trim_matches(is_js_whitespace).is_empty() {
        return None;
    }
    Some((name, params, rest))
}

fn classifier<'a>(scope: &'a Scope, locals: &'a [String]) -> impl Fn(&str) -> Kind + 'a {
    move |name: &str| {
        if locals.iter().any(|l| l == name) {
            Kind::Variable
        } else if scope.functions.contains_key(name) {
            Kind::Function
        } else if scope.variables.contains_key(name) {
            Kind::Variable
        } else if constant(name).is_some() {
            Kind::Constant
        } else if unit_named(name).is_some() {
            Kind::Unit
        } else {
            Kind::Variable
        }
    }
}

fn evaluate_line(line: &str, scope: &mut Scope) -> Result<CalcLine> {
    let trimmed = line.trim_matches(is_js_whitespace).to_owned();
    if let Some((name, params, rest)) = definition(line) {
        if !is_identifier(&name) {
            return fail(format!("“{name}” can't be used as a name."));
        }
        let params: Option<Vec<String>> = params.map(|p| {
            p.split(',')
                .map(|x| x.trim_matches(is_js_whitespace).to_owned())
                .filter(|x| !x.is_empty())
                .collect()
        });
        if params
            .as_ref()
            .is_some_and(|p| p.iter().any(|x| !is_identifier(x)))
        {
            return fail("Function parameters must be simple names.");
        }
        let (expression, show, unit) = split_evaluation(&rest)?;
        let body = parse_expression(&expression)?;
        let locals = params.clone().unwrap_or_default();
        let body_latex = to_latex(&body.node, &body.chains, &classifier(scope, &locals));
        let lhs = match &params {
            Some(p) => format!(
                "{}\\left({}\\right)",
                latex_identifier(&name, Kind::Function),
                p.iter()
                    .map(|x| latex_identifier(x, Kind::Variable))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            None => latex_identifier(&name, Kind::Variable),
        };
        if let Some(params) = params {
            if show {
                return fail("A function definition can't show a value.");
            }
            scope
                .functions
                .insert(name.clone(), UserFunction { params, body });
            scope.variables.remove(&name);
            return Ok(CalcLine::Math {
                source: line.to_owned(),
                latex: format!("{lhs} \\mathrel{{:=}} {body_latex}"),
                plain: trimmed,
            });
        }
        let value = evaluate(&body.node, &body.chains, scope, &Locals::new(), 0)?;
        scope.variables.insert(name.clone(), value);
        scope.functions.remove(&name);
        let latex = format!("{lhs} \\mathrel{{:=}} {body_latex}");
        if !show {
            return Ok(CalcLine::Math {
                source: line.to_owned(),
                latex,
                plain: trimmed,
            });
        }
        let none: Vec<String> = Vec::new();
        let classify = classifier(scope, &none);
        // `classify` borrows the scope; evaluate the display unit with a copy of it.
        let (result_latex, result_plain) = {
            let mut copy = Scope {
                variables: scope.variables.clone(),
                functions: scope.functions.clone(),
                calls: scope.calls,
            };
            format_result(value, &unit, &mut copy, &classify)?
        };
        // `line.slice(0, line.indexOf("=", line.indexOf(":=") + 2))`.
        let from = line.find(":=").map_or(0, |i| i + 2);
        let before = line[from..].find('=').map_or(line.len(), |i| from + i);
        return Ok(CalcLine::Math {
            source: line.to_owned(),
            latex: format!("{latex} = {result_latex}"),
            plain: format!(
                "{} = {result_plain}",
                line[..before].trim_matches(is_js_whitespace)
            ),
        });
    }
    let (expression, show, unit) = split_evaluation(line)?;
    let node = parse_expression(&expression)?;
    let none: Vec<String> = Vec::new();
    let latex = {
        let classify = classifier(scope, &none);
        to_latex(&node.node, &node.chains, &classify)
    };
    if !show {
        evaluate(&node.node, &node.chains, scope, &Locals::new(), 0)?;
        return Ok(CalcLine::Math {
            source: line.to_owned(),
            latex,
            plain: trimmed,
        });
    }
    let value = evaluate(&node.node, &node.chains, scope, &Locals::new(), 0)?;
    let mut copy = Scope {
        variables: scope.variables.clone(),
        functions: scope.functions.clone(),
        calls: scope.calls,
    };
    let classify = classifier(scope, &none);
    let (result_latex, result_plain) = format_result(value, &unit, &mut copy, &classify)?;
    Ok(CalcLine::Math {
        source: line.to_owned(),
        latex: format!("{latex} = {result_latex}"),
        plain: format!(
            "{} = {result_plain}",
            expression.trim_matches(is_js_whitespace)
        ),
    })
}

/// `evaluateWorksheet`: every line in order; errors stay on their own line.
pub fn evaluate_worksheet(source: &str) -> std::result::Result<Vec<CalcLine>, CalcError> {
    let text = source.trim_matches(is_js_whitespace);
    if text.is_empty() {
        return fail("Add a calculation first.");
    }
    if text.encode_utf16().count() > MAX_SOURCE_LENGTH {
        return fail("That calculation is too large to render.");
    }
    let raw: Vec<&str> = text
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect();
    if raw.len() > MAX_LINES {
        return fail(format!("Keep a calculation to {MAX_LINES} lines or fewer."));
    }
    let mut scope = Scope::default();
    Ok(raw
        .into_iter()
        .map(|line| {
            let trimmed = line.trim_matches(is_js_whitespace);
            if trimmed.is_empty() {
                return CalcLine::Blank;
            }
            if trimmed.starts_with('#') {
                return CalcLine::Text(
                    trimmed
                        .trim_start_matches('#')
                        .trim_start_matches(is_js_whitespace)
                        .to_owned(),
                );
            }
            scope.calls = 0;
            evaluate_line(line, &mut scope).unwrap_or_else(|CalcError(message)| CalcLine::Error {
                source: trimmed.to_owned(),
                message,
            })
        })
        .collect())
}

/// `worksheetToLatex`: one array, a row per line.
pub fn worksheet_to_latex(lines: &[CalcLine]) -> std::result::Result<String, CalcError> {
    let mut rows: Vec<String> = Vec::new();
    for line in lines {
        match line {
            CalcLine::Blank => {
                if rows.last().is_some_and(|r| !r.is_empty()) {
                    rows.push(String::new());
                }
            }
            CalcLine::Text(text) => rows.push(format!("\\text{{{}}}", escape_text(text))),
            CalcLine::Math { latex, .. } => rows.push(format!("\\displaystyle {latex}")),
            CalcLine::Error { source, message } => rows.push(format!(
                "\\text{{{}}}\\quad\\color{{#c0392b}}{{\\text{{{}}}}}",
                escape_text(source),
                escape_text(message)
            )),
        }
    }
    while rows.last().is_some_and(String::is_empty) {
        rows.pop();
    }
    if rows.is_empty() {
        return fail("Add a calculation first.");
    }
    Ok(format!(
        "\\begin{{array}}{{l}}{}\\end{{array}}",
        rows.join("\\\\[0.4em]")
    ))
}

/// `worksheetToPlainText`: the evaluated worksheet as text (or its source when it can't be).
pub fn worksheet_to_plain_text(source: &str) -> String {
    let Ok(lines) = evaluate_worksheet(source) else {
        return source.trim_matches(is_js_whitespace).to_owned();
    };
    let text = lines
        .iter()
        .map(|line| match line {
            CalcLine::Blank => String::new(),
            CalcLine::Text(t) => t.clone(),
            CalcLine::Math { plain, .. } => plain.clone(),
            CalcLine::Error { source, message } => format!("{source}  [{message}]"),
        })
        .collect::<Vec<_>>()
        .join("\n");
    // `.replace(/\n{3,}/g, "\n\n").trim()`.
    let mut out = String::new();
    let mut newlines = 0;
    for c in text.chars() {
        if c == '\n' {
            newlines += 1;
            if newlines <= 2 {
                out.push(c);
            }
        } else {
            newlines = 0;
            out.push(c);
        }
    }
    out.trim_matches(is_js_whitespace).to_owned()
}
