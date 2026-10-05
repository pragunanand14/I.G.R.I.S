//! `calculator` — exact arithmetic via a small recursive-descent parser.
//!
//! No code is ever evaluated: the input is tokenised and parsed against a
//! fixed grammar of numbers, operators, constants and whitelisted functions.

use serde_json::{json, Value};

use super::{PermissionLevel, Tool, ToolError, ToolOutput, ToolResultT, ToolSpec};

const MAX_LEN: usize = 300;
const MAX_DEPTH: usize = 64;

pub struct CalculatorTool {
    spec: ToolSpec,
}

impl Default for CalculatorTool {
    fn default() -> Self {
        Self {
            spec: ToolSpec {
                name: "calculator",
                title: "Calculator",
                description: "Evaluate a math expression exactly. Use this for any arithmetic instead of computing in your head. \
Supports + - * / % ^ (power), parentheses, factorial (!), constants pi and e, and functions sqrt, cbrt, abs, \
sin, cos, tan, asin, acos, atan, sinh, cosh, tanh, ln, log (base 10, or log(x, base)), log2, exp, floor, ceil, \
round, trunc, min, max, pow, hypot, deg (radians to degrees), rad (degrees to radians). Trigonometry uses radians.",
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "expression": { "type": "string", "minLength": 1, "maxLength": MAX_LEN, "description": "The expression, e.g. \"(3.5 + 2) * 4^2 / sqrt(2)\"" }
                    },
                    "required": ["expression"],
                    "additionalProperties": false
                }),
                permission: PermissionLevel::Safe,
            },
        }
    }
}

#[async_trait::async_trait]
impl Tool for CalculatorTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn describe(&self, input: &Value) -> String {
        format!("Calculate {}", input["expression"].as_str().unwrap_or_default().trim())
    }

    async fn execute(&self, input: &Value) -> ToolResultT {
        let expr = input["expression"].as_str().unwrap_or_default().trim();
        let value = evaluate(expr).map_err(ToolError::invalid)?;
        let formatted = format_number(value);
        Ok(ToolOutput { content: format!("{expr} = {formatted}"), summary: format!("= {formatted}"), sources: vec![], media: Vec::new() })
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64),
    Ident(String),
    Op(char),
    LParen,
    RParen,
    Comma,
}

fn tokenize(s: &str) -> Result<Vec<Tok>, String> {
    let chars: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            c if c.is_whitespace() => i += 1,
            '0'..='9' | '.' => {
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.' || chars[i] == '_') {
                    i += 1;
                }
                // Scientific notation: 1e3, 2.5E-4
                if i < chars.len() && (chars[i] == 'e' || chars[i] == 'E') {
                    let mut j = i + 1;
                    if j < chars.len() && (chars[j] == '+' || chars[j] == '-') {
                        j += 1;
                    }
                    if j < chars.len() && chars[j].is_ascii_digit() {
                        i = j;
                        while i < chars.len() && chars[i].is_ascii_digit() {
                            i += 1;
                        }
                    }
                }
                let text: String = chars[start..i].iter().filter(|c| **c != '_').collect();
                let n: f64 = text.parse().map_err(|_| format!("Invalid number '{text}'"))?;
                out.push(Tok::Num(n));
            }
            c if c.is_alphabetic() => {
                let start = i;
                while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                out.push(Tok::Ident(chars[start..i].iter().collect::<String>().to_lowercase()));
            }
            'π' => {
                out.push(Tok::Ident("pi".into()));
                i += 1;
            }
            '*' if chars.get(i + 1) == Some(&'*') => {
                out.push(Tok::Op('^'));
                i += 2;
            }
            '+' | '-' | '*' | '/' | '%' | '^' | '!' => {
                out.push(Tok::Op(c));
                i += 1;
            }
            '×' | '·' => {
                out.push(Tok::Op('*'));
                i += 1;
            }
            '÷' => {
                out.push(Tok::Op('/'));
                i += 1;
            }
            '−' => {
                out.push(Tok::Op('-'));
                i += 1;
            }
            '(' | '[' => {
                out.push(Tok::LParen);
                i += 1;
            }
            ')' | ']' => {
                out.push(Tok::RParen);
                i += 1;
            }
            ',' => {
                out.push(Tok::Comma);
                i += 1;
            }
            other => return Err(format!("Unexpected character '{other}'")),
        }
    }
    Ok(out)
}

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
    depth: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).cloned();
        self.pos += 1;
        t
    }

    fn enter(&mut self) -> Result<(), String> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err("Expression is nested too deeply".into());
        }
        Ok(())
    }

    fn expr(&mut self) -> Result<f64, String> {
        self.enter()?;
        let mut v = self.term()?;
        while let Some(Tok::Op(op @ ('+' | '-'))) = self.peek().cloned() {
            self.pos += 1;
            let r = self.term()?;
            v = if op == '+' { v + r } else { v - r };
        }
        self.depth -= 1;
        Ok(v)
    }

    fn term(&mut self) -> Result<f64, String> {
        let mut v = self.unary()?;
        while let Some(Tok::Op(op @ ('*' | '/' | '%'))) = self.peek().cloned() {
            self.pos += 1;
            let r = self.unary()?;
            v = match op {
                '*' => v * r,
                '/' if r == 0.0 => return Err("Division by zero".into()),
                '/' => v / r,
                _ if r == 0.0 => return Err("Modulo by zero".into()),
                _ => v % r,
            };
        }
        Ok(v)
    }

    fn unary(&mut self) -> Result<f64, String> {
        match self.peek() {
            Some(Tok::Op('-')) => {
                self.pos += 1;
                self.enter()?;
                let v = -self.unary()?;
                self.depth -= 1;
                Ok(v)
            }
            Some(Tok::Op('+')) => {
                self.pos += 1;
                self.unary()
            }
            _ => self.power(),
        }
    }

    fn power(&mut self) -> Result<f64, String> {
        let base = self.postfix()?;
        if let Some(Tok::Op('^')) = self.peek() {
            self.pos += 1;
            self.enter()?;
            let exp = self.unary()?; // right-associative
            self.depth -= 1;
            return Ok(base.powf(exp));
        }
        Ok(base)
    }

    fn postfix(&mut self) -> Result<f64, String> {
        let mut v = self.primary()?;
        while let Some(Tok::Op('!')) = self.peek() {
            self.pos += 1;
            v = factorial(v)?;
        }
        Ok(v)
    }

    fn primary(&mut self) -> Result<f64, String> {
        match self.next() {
            Some(Tok::Num(n)) => Ok(n),
            Some(Tok::LParen) => {
                let v = self.expr()?;
                match self.next() {
                    Some(Tok::RParen) => Ok(v),
                    _ => Err("Missing closing parenthesis".into()),
                }
            }
            Some(Tok::Ident(name)) => {
                if let Some(Tok::LParen) = self.peek() {
                    self.pos += 1;
                    let mut args = Vec::new();
                    if self.peek() != Some(&Tok::RParen) {
                        loop {
                            args.push(self.expr()?);
                            match self.next() {
                                Some(Tok::Comma) => continue,
                                Some(Tok::RParen) => break,
                                _ => return Err(format!("Expected ',' or ')' in {name}(…)")),
                            }
                        }
                    } else {
                        self.pos += 1;
                    }
                    call(&name, &args)
                } else {
                    match name.as_str() {
                        "pi" => Ok(std::f64::consts::PI),
                        "e" => Ok(std::f64::consts::E),
                        "tau" => Ok(std::f64::consts::TAU),
                        _ => Err(format!("Unknown name '{name}'")),
                    }
                }
            }
            Some(t) => Err(format!("Unexpected {}", describe_tok(&t))),
            None => Err("Unexpected end of expression".into()),
        }
    }
}

fn describe_tok(t: &Tok) -> String {
    match t {
        Tok::Op(c) => format!("'{c}'"),
        Tok::RParen => "')'".into(),
        Tok::Comma => "','".into(),
        _ => "token".into(),
    }
}

fn factorial(v: f64) -> Result<f64, String> {
    if v < 0.0 || v.fract() != 0.0 {
        return Err("Factorial is only defined for non-negative integers".into());
    }
    if v > 170.0 {
        return Err("Factorial result is too large".into());
    }
    Ok((1..=v as u64).fold(1.0, |acc, n| acc * n as f64))
}

fn call(name: &str, a: &[f64]) -> Result<f64, String> {
    let one = |f: fn(f64) -> f64| -> Result<f64, String> {
        match a {
            [x] => Ok(f(*x)),
            _ => Err(format!("{name}() takes exactly one argument")),
        }
    };
    let two = |f: fn(f64, f64) -> f64| -> Result<f64, String> {
        match a {
            [x, y] => Ok(f(*x, *y)),
            _ => Err(format!("{name}() takes exactly two arguments")),
        }
    };
    match name {
        "sqrt" => match a {
            [x] if *x < 0.0 => Err("Square root of a negative number".into()),
            _ => one(f64::sqrt),
        },
        "cbrt" => one(f64::cbrt),
        "abs" => one(f64::abs),
        "sin" => one(f64::sin),
        "cos" => one(f64::cos),
        "tan" => one(f64::tan),
        "asin" => one(f64::asin),
        "acos" => one(f64::acos),
        "atan" => one(f64::atan),
        "sinh" => one(f64::sinh),
        "cosh" => one(f64::cosh),
        "tanh" => one(f64::tanh),
        "ln" => one(f64::ln),
        "log" => match a {
            [x] => Ok(x.log10()),
            [x, b] => Ok(x.log(*b)),
            _ => Err("log() takes one or two arguments".into()),
        },
        "log10" => one(f64::log10),
        "log2" => one(f64::log2),
        "exp" => one(f64::exp),
        "floor" => one(f64::floor),
        "ceil" => one(f64::ceil),
        "round" => one(f64::round),
        "trunc" => one(f64::trunc),
        "deg" => one(f64::to_degrees),
        "rad" => one(f64::to_radians),
        "pow" => two(f64::powf),
        "hypot" => two(f64::hypot),
        "min" | "max" if a.is_empty() => Err(format!("{name}() needs at least one argument")),
        "min" => Ok(a.iter().cloned().fold(f64::INFINITY, f64::min)),
        "max" => Ok(a.iter().cloned().fold(f64::NEG_INFINITY, f64::max)),
        _ => Err(format!("Unknown function '{name}'")),
    }
}

pub fn evaluate(expr: &str) -> Result<f64, String> {
    if expr.chars().count() > MAX_LEN {
        return Err(format!("Expression is longer than {MAX_LEN} characters"));
    }
    let toks = tokenize(expr)?;
    if toks.is_empty() {
        return Err("Expression is empty".into());
    }
    let mut p = Parser { toks, pos: 0, depth: 0 };
    let v = p.expr()?;
    if p.pos < p.toks.len() {
        return Err(format!("Unexpected {} after the expression", describe_tok(&p.toks[p.pos])));
    }
    if v.is_nan() {
        return Err("The result is undefined".into());
    }
    if v.is_infinite() {
        return Err("The result is too large".into());
    }
    Ok(v)
}

pub fn format_number(v: f64) -> String {
    if v == 0.0 {
        return "0".into();
    }
    if v.fract().abs() < 1e-9 * v.abs().max(1.0) && v.abs() < 1e15 {
        return format!("{}", v.round() as i64);
    }
    if v.abs() >= 1e15 || v.abs() < 1e-6 {
        let s = format!("{v:.11e}");
        let (mantissa, exp) = s.split_once('e').unwrap_or((&s, "0"));
        let mantissa = mantissa.trim_end_matches('0').trim_end_matches('.');
        return format!("{mantissa}e{exp}");
    }
    // 12 significant digits, trailing zeros trimmed (hides float noise like 0.30000000000000004).
    let magnitude = v.abs().log10().floor() as i32 + 1; // digits before the point (≤ 0 for |v| < 1)
    let decimals = (12 - magnitude).max(0) as usize;
    let s = format!("{v:.decimals$}");
    if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(s: &str) -> f64 {
        evaluate(s).unwrap_or_else(|e| panic!("{s}: {e}"))
    }

    #[test]
    fn arithmetic_and_precedence() {
        assert_eq!(ev("2 + 3 * 4"), 14.0);
        assert_eq!(ev("(2 + 3) * 4"), 20.0);
        assert_eq!(ev("2 ^ 3 ^ 2"), 512.0);
        assert_eq!(ev("-2 ^ 2"), -4.0);
        assert_eq!(ev("2 ^ -1"), 0.5);
        assert_eq!(ev("10 % 3"), 1.0);
        assert_eq!(ev("5!"), 120.0);
        assert_eq!(ev("2**10"), 1024.0);
        assert_eq!(ev("6 × 7"), 42.0);
        assert_eq!(ev("1_000_000 / 1e3"), 1000.0);
        assert_eq!(ev("2.5E-1"), 0.25);
    }

    #[test]
    fn functions_and_constants() {
        assert_eq!(ev("sqrt(144)"), 12.0);
        assert!((ev("sin(pi / 2)") - 1.0).abs() < 1e-12);
        assert_eq!(ev("max(1, 7, 3)"), 7.0);
        assert_eq!(ev("log(1000)"), 3.0);
        assert_eq!(ev("log(8, 2)"), 3.0);
        assert!((ev("e") - std::f64::consts::E).abs() < 1e-15);
        assert_eq!(ev("round(deg(pi))"), 180.0);
    }

    #[test]
    fn rejects_bad_input() {
        for bad in [
            "", "1 +", "(1 + 2", "1 / 0", "5 % 0", "sqrt(-1)", "foo(2)", "x + 1", "2 2", "1; drop table",
            "import os", "__import__('os')", "(-1)!", "171!", "10^400", "0/0",
        ] {
            assert!(evaluate(bad).is_err(), "should reject {bad:?}");
        }
        assert!(evaluate(&"(".repeat(100)).is_err());
        assert!(evaluate(&"1+".repeat(200)).is_err());
    }

    #[test]
    fn formats_results() {
        assert_eq!(format_number(42.0), "42");
        assert_eq!(format_number(0.1 + 0.2), "0.3");
        assert_eq!(format_number(1.0 / 3.0), "0.333333333333");
        assert_eq!(format_number(-2.5), "-2.5");
        assert_eq!(format_number(1e20), "1e20");
        assert_eq!(format_number(123456.789), "123456.789");
        assert_eq!(format_number(1.5e-7), "1.5e-7");
    }

    #[tokio::test]
    async fn tool_returns_expression_and_result() {
        let out = CalculatorTool::default().execute(&serde_json::json!({"expression":"2*21"})).await.unwrap();
        assert_eq!(out.content, "2*21 = 42");
        assert_eq!(out.summary, "= 42");
    }
}
