use std::collections::HashMap;

use anyhow::{Context, bail};

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Number(f64),
    Identifier(String),
    Operator(String),
    LeftParen,
    RightParen,
    Comma,
    Question,
    Colon,
    End,
}

pub(crate) fn evaluate(formula: &str, variables: &HashMap<&str, f64>) -> anyhow::Result<f64> {
    let tokens = tokenize(formula)?;
    let mut parser = Parser {
        tokens: &tokens,
        position: 0,
        variables,
    };
    let result = parser.expression(0)?;
    if parser.peek() != &Token::End {
        bail!("unexpected token {:?} in formula", parser.peek());
    }
    if !result.is_finite() {
        bail!("formula produced a non-finite result");
    }
    Ok(result)
}

struct Parser<'a> {
    tokens: &'a [Token],
    position: usize,
    variables: &'a HashMap<&'a str, f64>,
}

impl Parser<'_> {
    fn peek(&self) -> &Token {
        &self.tokens[self.position]
    }

    fn take(&mut self) -> Token {
        let token = self.tokens[self.position].clone();
        self.position += 1;
        token
    }

    fn expression(&mut self, minimum_precedence: u8) -> anyhow::Result<f64> {
        let mut left = self.unary()?;
        loop {
            if self.peek() == &Token::Question && minimum_precedence <= 1 {
                self.take();
                let when_true = self.expression(0)?;
                if self.take() != Token::Colon {
                    bail!("expected ':' in conditional formula expression");
                }
                let when_false = self.expression(1)?;
                left = if left != 0.0 { when_true } else { when_false };
                continue;
            }

            let Token::Operator(operator) = self.peek() else {
                break;
            };
            let Some(precedence) = precedence(operator) else {
                break;
            };
            if precedence < minimum_precedence {
                break;
            }
            let operator = operator.clone();
            self.take();
            let right = self.expression(precedence + 1)?;
            left = apply_operator(&operator, left, right)?;
        }
        Ok(left)
    }

    fn unary(&mut self) -> anyhow::Result<f64> {
        if let Token::Operator(operator) = self.peek()
            && ["+", "-", "!", "~"].contains(&operator.as_str())
        {
            let operator = operator.clone();
            self.take();
            let value = self.unary()?;
            return match operator.as_str() {
                "+" => Ok(value),
                "-" => Ok(-value),
                "!" => Ok(bool_value(value == 0.0)),
                "~" => Ok((!integer(value)?) as f64),
                _ => unreachable!(),
            };
        }
        self.primary()
    }

    fn primary(&mut self) -> anyhow::Result<f64> {
        match self.take() {
            Token::Number(value) => Ok(value),
            Token::Identifier(identifier) => {
                if self.peek() == &Token::LeftParen {
                    self.take();
                    let mut arguments = Vec::new();
                    if self.peek() != &Token::RightParen {
                        loop {
                            arguments.push(self.expression(0)?);
                            if self.peek() != &Token::Comma {
                                break;
                            }
                            self.take();
                        }
                    }
                    if self.take() != Token::RightParen {
                        bail!("expected ')' after function arguments");
                    }
                    call_function(&identifier, &arguments)
                } else {
                    match identifier.as_str() {
                        "M_PI" | "PI" => Ok(std::f64::consts::PI),
                        _ => self
                            .variables
                            .get(identifier.as_str())
                            .copied()
                            .with_context(|| format!("formula references unknown counter '{identifier}'")),
                    }
                }
            }
            Token::LeftParen => {
                if let Some(cast_type) = self.read_cast_type() {
                    let value = self.unary()?;
                    return if cast_type
                        .split_whitespace()
                        .any(|word| matches!(word, "float" | "double"))
                    {
                        Ok(value)
                    } else {
                        Ok(value.trunc())
                    };
                }
                let value = self.expression(0)?;
                if self.take() != Token::RightParen {
                    bail!("expected ')' in formula");
                }
                Ok(value)
            }
            other => bail!("unexpected token {other:?} in formula"),
        }
    }

    fn read_cast_type(&mut self) -> Option<String> {
        let start = self.position;
        let mut words = Vec::new();
        while let Token::Identifier(word) = self.tokens.get(self.position)? {
            if !matches!(
                word.as_str(),
                "unsigned"
                    | "signed"
                    | "long"
                    | "short"
                    | "int"
                    | "float"
                    | "double"
                    | "char"
                    | "size_t"
                    | "uint64_t"
                    | "int64_t"
                    | "uint32_t"
                    | "int32_t"
            ) {
                break;
            }
            words.push(word.clone());
            self.position += 1;
        }
        if words.is_empty() || self.peek() != &Token::RightParen {
            self.position = start;
            return None;
        }
        self.take();
        Some(words.join(" "))
    }
}

fn precedence(operator: &str) -> Option<u8> {
    Some(match operator {
        "||" => 2,
        "&&" => 3,
        "|" => 4,
        "^" => 5,
        "&" => 6,
        "==" | "!=" => 7,
        "<" | "<=" | ">" | ">=" => 8,
        "<<" | ">>" => 9,
        "+" | "-" => 10,
        "*" | "/" | "%" => 11,
        _ => return None,
    })
}

fn apply_operator(operator: &str, left: f64, right: f64) -> anyhow::Result<f64> {
    Ok(match operator {
        "||" => bool_value(left != 0.0 || right != 0.0),
        "&&" => bool_value(left != 0.0 && right != 0.0),
        "|" => (integer(left)? | integer(right)?) as f64,
        "^" => (integer(left)? ^ integer(right)?) as f64,
        "&" => (integer(left)? & integer(right)?) as f64,
        "==" => bool_value(left == right),
        "!=" => bool_value(left != right),
        "<" => bool_value(left < right),
        "<=" => bool_value(left <= right),
        ">" => bool_value(left > right),
        ">=" => bool_value(left >= right),
        "<<" => integer(left)?
            .checked_shl(shift_amount(right)?)
            .context("invalid left shift in formula")? as f64,
        ">>" => integer(left)?
            .checked_shr(shift_amount(right)?)
            .context("invalid right shift in formula")? as f64,
        "+" => left + right,
        "-" => left - right,
        "*" => left * right,
        "/" => left / right,
        "%" => left % right,
        _ => bail!("unsupported operator '{operator}'"),
    })
}

fn call_function(name: &str, arguments: &[f64]) -> anyhow::Result<f64> {
    let unary = || {
        if let [value] = arguments {
            Ok(*value)
        } else {
            bail!("function '{name}' expects one argument")
        }
    };
    let binary = || {
        if let [left, right] = arguments {
            Ok((*left, *right))
        } else {
            bail!("function '{name}' expects two arguments")
        }
    };
    match name {
        "min" | "MIN" | "fmin" | "fminf" => {
            let (left, right) = binary()?;
            Ok(left.min(right))
        }
        "max" | "MAX" | "fmax" | "fmaxf" => {
            let (left, right) = binary()?;
            Ok(left.max(right))
        }
        "abs" | "fabs" | "fabsf" => Ok(unary()?.abs()),
        "sqrt" | "sqrtf" => Ok(unary()?.sqrt()),
        "log2" => Ok(unary()?.log2()),
        "log" | "logf" => Ok(unary()?.ln()),
        "exp" | "expf" => Ok(unary()?.exp()),
        "pow" | "powf" => {
            let (left, right) = binary()?;
            Ok(left.powf(right))
        }
        "ceil" | "ceilf" => Ok(unary()?.ceil()),
        "floor" | "floorf" => Ok(unary()?.floor()),
        "round" | "roundf" => Ok(unary()?.round()),
        _ => bail!("unsupported function '{name}' in LIKWID formula"),
    }
}

fn bool_value(value: bool) -> f64 {
    if value { 1.0 } else { 0.0 }
}

fn integer(value: f64) -> anyhow::Result<i64> {
    if !value.is_finite() || value.fract() != 0.0 || value < i64::MIN as f64 || value > i64::MAX as f64 {
        bail!("bitwise formula operator requires an integer value, got {value}");
    }
    Ok(value as i64)
}

fn shift_amount(value: f64) -> anyhow::Result<u32> {
    let amount = integer(value)?;
    u32::try_from(amount).context("invalid shift amount")
}

fn tokenize(formula: &str) -> anyhow::Result<Vec<Token>> {
    let bytes = formula.as_bytes();
    let mut position = 0;
    let mut tokens = Vec::new();
    while position < bytes.len() {
        let byte = bytes[position];
        if byte.is_ascii_whitespace() {
            position += 1;
            continue;
        }
        if byte.is_ascii_digit() || (byte == b'.' && bytes.get(position + 1).is_some_and(u8::is_ascii_digit)) {
            let start = position;
            if byte == b'0' && bytes.get(position + 1).is_some_and(|b| matches!(b, b'x' | b'X')) {
                position += 2;
                while bytes.get(position).is_some_and(u8::is_ascii_hexdigit) {
                    position += 1;
                }
                let digits = &formula[start + 2..position];
                let value = u64::from_str_radix(digits, 16)
                    .with_context(|| format!("invalid hexadecimal number in formula: {digits}"))?;
                tokens.push(Token::Number(value as f64));
            } else {
                position = scan_decimal_number(bytes, position);
                let raw = &formula[start..position];
                let cleaned = raw.trim_end_matches(['u', 'U', 'l', 'L']);
                let value = cleaned
                    .parse::<f64>()
                    .with_context(|| format!("invalid number in formula: {raw}"))?;
                tokens.push(Token::Number(value));
            }
            while bytes
                .get(position)
                .is_some_and(|b| matches!(b, b'u' | b'U' | b'l' | b'L'))
            {
                position += 1;
            }
            continue;
        }
        if byte.is_ascii_alphabetic() || byte == b'_' {
            let start = position;
            position += 1;
            while bytes
                .get(position)
                .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
            {
                position += 1;
            }
            tokens.push(Token::Identifier(formula[start..position].to_owned()));
            continue;
        }

        let remaining = &formula[position..];
        let operator = ["||", "&&", "<<", ">>", "<=", ">=", "==", "!="]
            .iter()
            .find(|operator| remaining.starts_with(**operator));
        if let Some(operator) = operator {
            tokens.push(Token::Operator((*operator).to_owned()));
            position += operator.len();
            continue;
        }
        match byte {
            b'+' | b'-' | b'*' | b'/' | b'%' | b'<' | b'>' | b'&' | b'|' | b'^' | b'!' | b'~' => {
                tokens.push(Token::Operator(char::from(byte).to_string()));
            }
            b'(' => tokens.push(Token::LeftParen),
            b')' => tokens.push(Token::RightParen),
            b',' => tokens.push(Token::Comma),
            b'?' => tokens.push(Token::Question),
            b':' => tokens.push(Token::Colon),
            _ => bail!("unsupported character '{}' in formula", char::from(byte)),
        }
        position += 1;
    }
    tokens.push(Token::End);
    Ok(tokens)
}

fn scan_decimal_number(bytes: &[u8], mut position: usize) -> usize {
    while bytes.get(position).is_some_and(|b| b.is_ascii_digit() || *b == b'.') {
        position += 1;
    }
    if bytes.get(position).is_some_and(|b| matches!(b, b'e' | b'E')) {
        position += 1;
        if bytes.get(position).is_some_and(|b| matches!(b, b'+' | b'-')) {
            position += 1;
        }
        while bytes.get(position).is_some_and(u8::is_ascii_digit) {
            position += 1;
        }
    }
    position
}

#[cfg(test)]
#[path = "../tests/formula.rs"]
mod tests;
