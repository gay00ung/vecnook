//! Bounded typed attributes and Boolean document filters.
use crate::{Error, Result};
use std::{cmp::Ordering, fmt, ops::Bound, str::FromStr};

/// A finite f64 attribute, with numeric equality and a total order. Zero is canonicalized.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Number(f64);
impl Number {
    /// Reject NaN and infinities before storing or filtering numeric attributes.
    pub fn new(value: f64) -> Result<Self> {
        if !value.is_finite() {
            return Err(invalid("number must be finite"));
        }
        Ok(Self(if value == 0.0 { 0.0 } else { value }))
    }
    /// Original finite numeric value, with negative zero represented as zero.
    pub fn get(self) -> f64 {
        self.0
    }
}
impl Eq for Number {}
impl Ord for Number {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.total_cmp(&other.0)
    }
}
impl PartialOrd for Number {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl fmt::Display for Number {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// A Gregorian calendar date in years 0001..9999, at day precision.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Date(u32);
impl Date {
    /// Validate a Gregorian year, month and day, including leap-year rules.
    pub fn new(year: u32, month: u32, day: u32) -> Result<Self> {
        let leap =
            year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
        let days = match month {
            2 if leap => 29,
            2 => 28,
            4 | 6 | 9 | 11 => 30,
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            _ => 0,
        };
        if !(1..=9999).contains(&year) || day == 0 || day > days {
            return Err(invalid("invalid Gregorian date"));
        }
        Ok(Self(year * 10000 + month * 100 + day))
    }
    /// Parse exactly YYYY-MM-DD using ASCII digits.
    pub fn parse(value: &str) -> Result<Self> {
        value.parse()
    }
    pub(crate) fn encoded(self) -> u32 {
        self.0
    }
    pub(crate) fn decode(value: u32) -> Result<Self> {
        Self::new(value / 10000, value / 100 % 100, value % 100)
    }
}
impl FromStr for Date {
    type Err = Error;
    fn from_str(value: &str) -> Result<Self> {
        let bytes = value.as_bytes();
        if bytes.len() != 10
            || bytes[4] != b'-'
            || bytes[7] != b'-'
            || bytes
                .iter()
                .enumerate()
                .any(|(i, b)| i != 4 && i != 7 && !b.is_ascii_digit())
        {
            return Err(invalid("date must be YYYY-MM-DD"));
        }
        Self::new(
            value[..4].parse().unwrap(),
            value[5..7].parse().unwrap(),
            value[8..].parse().unwrap(),
        )
    }
}
impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:04}-{:02}-{:02}",
            self.0 / 10000,
            self.0 / 100 % 100,
            self.0 % 100
        )
    }
}

/// Owned, indexed document predicate. Search validates at most 256 nodes and 32 levels.
/// Empty And matches every active ID; empty Or matches none. Not complements active IDs,
/// including documents missing an attribute. A positive range requires the attribute.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FilterExpr {
    /// Exact source equality.
    Source(String),
    /// Membership in the document's tags.
    Tag(String),
    /// Intersection of every child predicate.
    And(Vec<FilterExpr>),
    /// Union of any child predicate.
    Or(Vec<FilterExpr>),
    /// Complement relative to currently active document IDs.
    Not(Box<FilterExpr>),
    /// Finite numeric interval; absent fields do not match.
    NumberRange {
        /// Attribute key, 1..128 UTF-8 bytes without NUL.
        field: String,
        /// Inclusive, exclusive or unbounded lower endpoint.
        lower: Bound<Number>,
        /// Inclusive, exclusive or unbounded upper endpoint.
        upper: Bound<Number>,
    },
    /// Gregorian date interval; absent fields do not match.
    DateRange {
        /// Attribute key, 1..128 UTF-8 bytes without NUL.
        field: String,
        /// Inclusive, exclusive or unbounded lower endpoint.
        lower: Bound<Date>,
        /// Inclusive, exclusive or unbounded upper endpoint.
        upper: Bound<Date>,
    },
}
impl FilterExpr {
    /// Match an exact source identifier.
    pub fn source(value: impl Into<String>) -> Self {
        Self::Source(value.into())
    }
    /// Match a tag.
    pub fn tag(value: impl Into<String>) -> Self {
        Self::Tag(value.into())
    }
    /// Require all children; no children means every active document.
    pub fn all(children: Vec<Self>) -> Self {
        Self::And(children)
    }
    /// Require any child; no children means no documents.
    pub fn any(children: Vec<Self>) -> Self {
        Self::Or(children)
    }
    /// Complement this predicate, including missing attributes.
    pub fn negated(self) -> Self {
        Self::Not(Box::new(self))
    }
    /// Select a numeric interval. Reversed bounds are rejected by validate/search.
    pub fn number_range(
        field: impl Into<String>,
        lower: Bound<Number>,
        upper: Bound<Number>,
    ) -> Self {
        Self::NumberRange {
            field: field.into(),
            lower,
            upper,
        }
    }
    /// Select a date interval. Reversed bounds are rejected by validate/search.
    pub fn date_range(field: impl Into<String>, lower: Bound<Date>, upper: Bound<Date>) -> Self {
        Self::DateRange {
            field: field.into(),
            lower,
            upper,
        }
    }
    /// Parse a UTF-8 expression of at most 16 KiB. Precedence: NOT, AND, OR.
    /// Functions are source("path"), tag("name"), number("key") and date("key").
    /// Numeric/date functions require =, ==, !=, <, <=, > or >= and a value;
    /// dates and strings use JSON quoting. Operators/functions ignore ASCII case.
    pub fn parse(input: &str) -> Result<Self> {
        if input.len() > 16384 {
            return Err(invalid("filter exceeds 16 KiB"));
        }
        let mut parser = Parser {
            input,
            offset: 0,
            nodes: 0,
        };
        let expression = parser.or(1)?;
        parser.space();
        if parser.offset != input.len() {
            return Err(invalid("unexpected filter input"));
        }
        expression.validate()?;
        Ok(expression)
    }
    /// Check keys, source/tag bounds, interval order and expression complexity.
    pub fn validate(&self) -> Result<()> {
        fn visit(expr: &FilterExpr, depth: usize, nodes: &mut usize) -> Result<()> {
            *nodes += 1;
            if depth > 32 || *nodes > 256 {
                return Err(invalid("filter exceeds 32 levels or 256 nodes"));
            }
            match expr {
                FilterExpr::Source(s) => text(s, 1024),
                FilterExpr::Tag(s) => text(s, 128),
                FilterExpr::And(children) | FilterExpr::Or(children) => {
                    if children.len() > 256 {
                        return Err(invalid("too many filter children"));
                    }
                    for child in children {
                        visit(child, depth + 1, nodes)?;
                    }
                    Ok(())
                }
                FilterExpr::Not(child) => visit(child, depth + 1, nodes),
                FilterExpr::NumberRange {
                    field,
                    lower,
                    upper,
                } => {
                    key(field)?;
                    bounds(lower, upper)
                }
                FilterExpr::DateRange {
                    field,
                    lower,
                    upper,
                } => {
                    key(field)?;
                    bounds(lower, upper)
                }
            }
        }
        visit(self, 1, &mut 0)
    }
}
pub(crate) fn key(value: &str) -> Result<()> {
    text(value, 128)
}
fn text(value: &str, max: usize) -> Result<()> {
    if value.is_empty() || value.len() > max || value.contains('\0') {
        Err(invalid("invalid filter/attribute key or value"))
    } else {
        Ok(())
    }
}
fn bounds<T: Ord>(lower: &Bound<T>, upper: &Bound<T>) -> Result<()> {
    if let (Bound::Included(a) | Bound::Excluded(a), Bound::Included(b) | Bound::Excluded(b)) =
        (lower, upper)
        && a > b
    {
        return Err(invalid("range lower bound exceeds upper bound"));
    }
    Ok(())
}
pub(crate) fn empty_range<T: Ord>(lower: &Bound<T>, upper: &Bound<T>) -> bool {
    match (lower, upper) {
        (Bound::Excluded(a), Bound::Included(b) | Bound::Excluded(b))
        | (Bound::Included(a), Bound::Excluded(b)) => a == b,
        _ => false,
    }
}
fn invalid(message: &str) -> Error {
    Error::InvalidInput(message.into())
}

struct Parser<'a> {
    input: &'a str,
    offset: usize,
    nodes: usize,
}
impl Parser<'_> {
    fn space(&mut self) {
        while self.peek().is_some_and(|c| c.is_ascii_whitespace()) {
            self.offset += 1;
        }
    }
    fn peek(&self) -> Option<u8> {
        self.input.as_bytes().get(self.offset).copied()
    }
    fn token(&mut self, token: &str) -> bool {
        self.space();
        let Some(end) = self.offset.checked_add(token.len()) else {
            return false;
        };
        if self
            .input
            .as_bytes()
            .get(self.offset..end)
            .is_some_and(|s| s.eq_ignore_ascii_case(token.as_bytes()))
            && !self
                .input
                .as_bytes()
                .get(end)
                .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
        {
            self.offset = end;
            true
        } else {
            false
        }
    }
    fn character(&mut self, value: u8) -> bool {
        self.space();
        if self.peek() == Some(value) {
            self.offset += 1;
            true
        } else {
            false
        }
    }
    fn expect(&mut self, value: u8) -> Result<()> {
        if self.character(value) {
            Ok(())
        } else {
            Err(invalid("missing filter punctuation"))
        }
    }
    fn node(&mut self, expr: FilterExpr) -> Result<FilterExpr> {
        self.nodes += 1;
        if self.nodes > 256 {
            return Err(invalid("filter exceeds 256 nodes"));
        }
        Ok(expr)
    }
    fn or(&mut self, depth: usize) -> Result<FilterExpr> {
        let first = self.and(depth)?;
        if !self.token("OR") {
            return Ok(first);
        }
        let mut children = vec![first, self.and(depth)?];
        while self.token("OR") {
            children.push(self.and(depth)?);
        }
        self.node(FilterExpr::Or(children))
    }
    fn and(&mut self, depth: usize) -> Result<FilterExpr> {
        let first = self.unary(depth)?;
        if !self.token("AND") {
            return Ok(first);
        }
        let mut children = vec![first, self.unary(depth)?];
        while self.token("AND") {
            children.push(self.unary(depth)?);
        }
        self.node(FilterExpr::And(children))
    }
    fn unary(&mut self, depth: usize) -> Result<FilterExpr> {
        if depth > 32 {
            return Err(invalid("filter exceeds 32 levels"));
        }
        if self.token("NOT") {
            let child = self.unary(depth + 1)?;
            return self.node(child.negated());
        }
        if self.character(b'(') {
            let expr = self.or(depth + 1)?;
            self.expect(b')')?;
            return Ok(expr);
        }
        let kind = if self.token("source") {
            0
        } else if self.token("tag") {
            1
        } else if self.token("number") {
            2
        } else if self.token("date") {
            3
        } else {
            return Err(invalid(
                "expected source, tag, number, date or parenthesized filter",
            ));
        };
        self.expect(b'(')?;
        let field = self.string()?;
        self.expect(b')')?;
        if kind == 0 {
            return self.node(FilterExpr::Source(field));
        }
        if kind == 1 {
            return self.node(FilterExpr::Tag(field));
        }
        self.space();
        let start = self.offset;
        while self.peek().is_some_and(|c| b"=<>!".contains(&c)) {
            self.offset += 1;
        }
        let operator = &self.input[start..self.offset];
        if !["=", "==", "!=", "<", "<=", ">", ">="].contains(&operator) {
            return Err(invalid("expected range comparison"));
        }
        fn interval<T: Copy>(operator: &str, value: T) -> (Bound<T>, Bound<T>) {
            use Bound::*;
            match operator {
                "<" => (Unbounded, Excluded(value)),
                "<=" => (Unbounded, Included(value)),
                ">" => (Excluded(value), Unbounded),
                ">=" => (Included(value), Unbounded),
                _ => (Included(value), Included(value)),
            }
        }
        let expr = if kind == 2 {
            self.space();
            let start = self.offset;
            while self
                .peek()
                .is_some_and(|c| !c.is_ascii_whitespace() && c != b')')
            {
                self.offset += 1;
            }
            let value = self.input[start..self.offset]
                .parse::<f64>()
                .map_err(|_| invalid("invalid finite number"))?;
            let (lower, upper) = interval(operator, Number::new(value)?);
            FilterExpr::NumberRange {
                field,
                lower,
                upper,
            }
        } else {
            let value = Date::parse(&self.string()?)?;
            let (lower, upper) = interval(operator, value);
            FilterExpr::DateRange {
                field,
                lower,
                upper,
            }
        };
        let expr = self.node(expr)?;
        if operator == "!=" {
            self.node(expr.negated())
        } else {
            Ok(expr)
        }
    }
    fn string(&mut self) -> Result<String> {
        self.expect(b'"')?;
        let mut out = String::new();
        loop {
            match self
                .peek()
                .ok_or_else(|| invalid("unterminated filter string"))?
            {
                b'"' => {
                    self.offset += 1;
                    return Ok(out);
                }
                b'\\' => {
                    self.offset += 1;
                    let escape = self.peek().ok_or_else(|| invalid("truncated escape"))?;
                    self.offset += 1;
                    out.push(match escape {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => {
                            let first = self.hex4()?;
                            let scalar = if (0xd800..=0xdbff).contains(&first) {
                                if self.input.as_bytes().get(self.offset..self.offset + 2)
                                    != Some(b"\\u")
                                {
                                    return Err(invalid("missing low surrogate"));
                                }
                                self.offset += 2;
                                let second = self.hex4()?;
                                if !(0xdc00..=0xdfff).contains(&second) {
                                    return Err(invalid("invalid low surrogate"));
                                }
                                0x10000 + ((first - 0xd800) << 10) + second - 0xdc00
                            } else {
                                first
                            };
                            char::from_u32(scalar)
                                .ok_or_else(|| invalid("invalid Unicode escape"))?
                        }
                        _ => return Err(invalid("invalid string escape")),
                    });
                }
                0..=31 => return Err(invalid("unescaped control character")),
                _ => {
                    let c = self.input[self.offset..].chars().next().unwrap();
                    self.offset += c.len_utf8();
                    out.push(c);
                }
            }
            if out.len() > 1024 {
                return Err(invalid("filter string exceeds 1024 bytes"));
            }
        }
    }
    fn hex4(&mut self) -> Result<u32> {
        let mut value = 0;
        for _ in 0..4 {
            let digit = self
                .peek()
                .and_then(|b| (b as char).to_digit(16))
                .ok_or_else(|| invalid("invalid Unicode escape"))?;
            self.offset += 1;
            value = value * 16 + digit;
        }
        Ok(value)
    }
}
