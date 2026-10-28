//! Small numeric formula subset; admission of preserved formulas never executes them.
use super::types::{invalid, unsupported, OfficeEditError, Result};
use std::collections::BTreeSet;

pub(super) type Coordinate = (u32, u32);
pub(super) fn coordinate(text: &str) -> Result<Coordinate> {
    let text = text.strip_prefix('$').unwrap_or(text);
    let split = text
        .find(|c: char| !c.is_ascii_alphabetic())
        .ok_or_else(|| invalid("cell address"))?;
    let (column, row) = text.split_at(split);
    let row = row.strip_prefix('$').unwrap_or(row);
    if column.is_empty()
        || column.len() > 3
        || !column.bytes().all(|c| c.is_ascii_alphabetic())
        || row.starts_with('0')
        || !row.bytes().all(|c| c.is_ascii_digit())
    {
        return Err(invalid("cell address"));
    }
    let column = column.bytes().fold(0u32, |n, c| {
        n * 26 + u32::from(c.to_ascii_uppercase() - b'A' + 1)
    });
    let row = row.parse::<u32>().map_err(|_| invalid("cell address"))?;
    if !(1..=16_384).contains(&column) || !(1..=1_048_576).contains(&row) {
        return Err(invalid("cell address is outside Excel bounds"));
    }
    Ok((row, column))
}
pub(super) fn address((row, mut column): Coordinate) -> String {
    let mut letters = Vec::new();
    while column > 0 {
        column -= 1;
        letters.push((b'A' + (column % 26) as u8) as char);
        column /= 26;
    }
    format!("{}{row}", letters.iter().rev().collect::<String>())
}
#[derive(Debug, Clone, PartialEq)]
enum Token {
    Number(f64),
    Name(String),
    Text,
    Symbol(char),
}
fn tokens(formula: &str) -> Result<Vec<Token>> {
    if formula.is_empty() || formula.len() > 4096 || formula.starts_with('=') {
        return Err(invalid("formula text or length"));
    }
    let chars = formula.chars().collect::<Vec<_>>();
    let mut i = 0;
    let mut out = Vec::new();
    while i < chars.len() {
        let c = chars[i];
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if c == '"' {
            i += 1;
            let mut closed = false;
            while i < chars.len() {
                if chars[i] == '"' {
                    if chars.get(i + 1) == Some(&'"') {
                        i += 2;
                        continue;
                    }
                    i += 1;
                    closed = true;
                    break;
                }
                if chars[i].is_control() {
                    return Err(invalid("formula string"));
                }
                i += 1;
            }
            if !closed {
                return Err(invalid("unterminated formula string"));
            }
            out.push(Token::Text);
        } else if c.is_ascii_digit()
            || c == '.' && chars.get(i + 1).is_some_and(char::is_ascii_digit)
        {
            let start = i;
            i += 1;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            let value = chars[start..i]
                .iter()
                .collect::<String>()
                .parse::<f64>()
                .map_err(|_| invalid("formula number"))?;
            if !value.is_finite() {
                return Err(invalid("nonfinite formula number"));
            }
            out.push(Token::Number(value));
        } else if c.is_ascii_alphabetic() || matches!(c, '_' | '$') {
            let start = i;
            i += 1;
            while i < chars.len()
                && (chars[i].is_ascii_alphanumeric() || matches!(chars[i], '_' | '.' | '$'))
            {
                i += 1;
            }
            out.push(Token::Name(
                chars[start..i]
                    .iter()
                    .collect::<String>()
                    .to_ascii_uppercase(),
            ));
        } else if "+-*/():,=<>".contains(c) {
            out.push(Token::Symbol(c));
            i += 1;
        } else {
            return Err(unsupported("formula construct or external reference"));
        }
    }
    if out.len() > 2048 {
        return Err(OfficeEditError::ResourceLimit);
    }
    Ok(out)
}
fn allowed_function(name: &str) -> bool {
    matches!(
        name,
        "SUM"
            | "AVERAGE"
            | "MIN"
            | "MAX"
            | "COUNT"
            | "IF"
            | "IFERROR"
            | "AND"
            | "OR"
            | "NOT"
            | "ABS"
            | "ROUND"
            | "ROUNDUP"
            | "ROUNDDOWN"
            | "SUMIF"
            | "SUMIFS"
            | "COUNTIF"
            | "COUNTIFS"
            | "VLOOKUP"
            | "HLOOKUP"
            | "XLOOKUP"
            | "INDEX"
            | "MATCH"
            | "CONCAT"
            | "CONCATENATE"
            | "LEFT"
            | "RIGHT"
            | "MID"
            | "LEN"
            | "TRIM"
            | "DATE"
            | "YEAR"
            | "MONTH"
            | "DAY"
            | "TEXT"
    )
}
pub(super) fn admit(formula: &str, names: &BTreeSet<String>) -> Result<()> {
    let values = tokens(formula)?;
    let mut depth = 0usize;
    for (i, token) in values.iter().enumerate() {
        match token {
            Token::Name(name) => {
                let canonical = name.strip_prefix("_XLFN.").unwrap_or(name);
                if values.get(i + 1) == Some(&Token::Symbol('(')) {
                    if !allowed_function(canonical) {
                        return Err(unsupported("unsafe or unknown formula function"));
                    }
                } else if coordinate(name).is_err()
                    && !matches!(name.as_str(), "TRUE" | "FALSE")
                    && !names.contains(name)
                {
                    return Err(unsupported("formula name or reference"));
                }
            }
            Token::Symbol('(') => {
                depth += 1;
                if depth > 64 {
                    return Err(OfficeEditError::ResourceLimit);
                }
            }
            Token::Symbol(')') => {
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| invalid("formula parentheses"))?;
            }
            _ => {}
        }
    }
    if depth != 0 {
        return Err(invalid("formula parentheses"));
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub(super) enum Expr {
    Number(f64),
    Cell(Coordinate),
    Range(Coordinate, Coordinate),
    Unary(char, Box<Expr>),
    Binary(char, Box<Expr>, Box<Expr>),
    Aggregate(String, Vec<Expr>),
}
struct Parser {
    tokens: Vec<Token>,
    at: usize,
    depth: usize,
}
pub(super) fn parse(formula: &str) -> Result<Expr> {
    let mut parser = Parser {
        tokens: tokens(formula)?,
        at: 0,
        depth: 0,
    };
    let result = parser.expression(0)?;
    if parser.at != parser.tokens.len() {
        return Err(unsupported("formula calculation subset"));
    }
    fn check(expr: &Expr, range_argument: bool) -> Result<()> {
        match expr {
            Expr::Range(..) if !range_argument => Err(unsupported(
                "ranges are only allowed as aggregate arguments",
            )),
            Expr::Unary(_, value) => check(value, false),
            Expr::Binary(_, left, right) => {
                check(left, false)?;
                check(right, false)
            }
            Expr::Aggregate(_, args) => {
                for arg in args {
                    check(arg, true)?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
    check(&result, false)?;
    Ok(result)
}
impl Parser {
    fn symbol(&mut self, c: char) -> bool {
        if self.tokens.get(self.at) == Some(&Token::Symbol(c)) {
            self.at += 1;
            true
        } else {
            false
        }
    }
    fn expression(&mut self, min: u8) -> Result<Expr> {
        self.depth += 1;
        if self.depth > 64 {
            return Err(OfficeEditError::ResourceLimit);
        }
        let token = self
            .tokens
            .get(self.at)
            .cloned()
            .ok_or_else(|| invalid("incomplete formula"))?;
        self.at += 1;
        let mut left = match token {
            Token::Number(value) => Expr::Number(value),
            Token::Symbol(op @ ('+' | '-')) => Expr::Unary(op, Box::new(self.expression(3)?)),
            Token::Symbol('(') => {
                let result = self.expression(0)?;
                if !self.symbol(')') {
                    return Err(invalid("formula parentheses"));
                }
                result
            }
            Token::Name(name) => {
                if self.symbol('(') {
                    if !matches!(name.as_str(), "SUM" | "AVERAGE" | "MIN" | "MAX" | "COUNT") {
                        return Err(unsupported("formula calculation subset"));
                    }
                    let mut args = Vec::new();
                    if !self.symbol(')') {
                        loop {
                            args.push(self.expression(0)?);
                            if self.symbol(')') {
                                break;
                            }
                            if !self.symbol(',') {
                                return Err(invalid("formula arguments"));
                            }
                        }
                    }
                    Expr::Aggregate(name, args)
                } else {
                    let first = coordinate(&name)?;
                    if self.symbol(':') {
                        let Some(Token::Name(last)) = self.tokens.get(self.at) else {
                            return Err(invalid("formula range"));
                        };
                        let last = coordinate(last)?;
                        self.at += 1;
                        if first.0 > last.0 || first.1 > last.1 {
                            return Err(invalid("reversed formula range"));
                        }
                        Expr::Range(first, last)
                    } else {
                        Expr::Cell(first)
                    }
                }
            }
            _ => return Err(unsupported("formula calculation subset")),
        };
        loop {
            let Some(Token::Symbol(op @ ('+' | '-' | '*' | '/'))) = self.tokens.get(self.at) else {
                break;
            };
            let op = *op;
            let precedence = if matches!(op, '*' | '/') { 2 } else { 1 };
            if precedence < min {
                break;
            }
            self.at += 1;
            let right = self.expression(precedence + 1)?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
        self.depth -= 1;
        Ok(left)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum Computed {
    Number(f64),
    Blank,
    Deferred,
    Error,
}
pub(super) fn evaluate(
    expr: &Expr,
    read: &mut impl FnMut(Coordinate) -> Computed,
    visits: &mut usize,
) -> Computed {
    fn number(value: Computed) -> std::result::Result<f64, Computed> {
        match value {
            Computed::Number(x) => Ok(x),
            Computed::Blank => Ok(0.0),
            other => Err(other),
        }
    }
    let result = match expr {
        Expr::Number(n) => *n,
        Expr::Cell(c) => return read(*c),
        Expr::Range(..) => return Computed::Deferred,
        Expr::Unary(op, value) => {
            let value = match number(evaluate(value, read, visits)) {
                Ok(v) => v,
                Err(e) => return e,
            };
            if *op == '-' {
                -value
            } else {
                value
            }
        }
        Expr::Binary(op, left, right) => {
            let left = match number(evaluate(left, read, visits)) {
                Ok(v) => v,
                Err(e) => return e,
            };
            let right = match number(evaluate(right, read, visits)) {
                Ok(v) => v,
                Err(e) => return e,
            };
            match op {
                '+' => left + right,
                '-' => left - right,
                '*' => left * right,
                '/' if right != 0.0 => left / right,
                _ => return Computed::Error,
            }
        }
        Expr::Aggregate(name, args) => {
            let mut values = Vec::new();
            for arg in args {
                if let Expr::Range(first, last) = arg {
                    let count = u64::from(last.0 - first.0 + 1) * u64::from(last.1 - first.1 + 1);
                    if count > 100_000 || (*visits as u64) + count > 100_000 {
                        return Computed::Deferred;
                    }
                    for row in first.0..=last.0 {
                        for column in first.1..=last.1 {
                            *visits += 1;
                            match read((row, column)) {
                                Computed::Number(v) => values.push(v),
                                Computed::Blank => {}
                                other => return other,
                            }
                        }
                    }
                } else {
                    match evaluate(arg, read, visits) {
                        Computed::Number(v) => values.push(v),
                        Computed::Blank => {}
                        other => return other,
                    }
                }
            }
            match name.as_str() {
                "SUM" => values.iter().sum(),
                "COUNT" => values.len() as f64,
                "AVERAGE" if !values.is_empty() => values.iter().sum::<f64>() / values.len() as f64,
                "MIN" => values.into_iter().reduce(f64::min).unwrap_or(0.0),
                "MAX" => values.into_iter().reduce(f64::max).unwrap_or(0.0),
                _ => return Computed::Error,
            }
        }
    };
    if result.is_finite() {
        Computed::Number(result)
    } else {
        Computed::Error
    }
}
