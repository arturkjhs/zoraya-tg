//! JSON as the protocol uses it (RFC 8259): a value parsed from one line, and the writer
//! of one. The parser is total on any bytes (a line from the socket is the backend's, but
//! it is still only bytes): it refuses what is not JSON, nests at most [`MAX_DEPTH`] deep,
//! and keeps numbers as `i64` or `f64`.

use alloc::string::String;
use alloc::vec::Vec;

/// Arrays and objects nest at most this deep.
pub const MAX_DEPTH: usize = 32;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Array(Vec<Value>),
    /// The members in order; a repeated key keeps its first value for `get`.
    Object(Vec<(String, Value)>),
}

impl Value {
    /// The member `key` of an object.
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Object(m) => m.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Int(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Array(a) => Some(a),
            _ => None,
        }
    }
}

/// The value `text` holds, with nothing but whitespace around it.
pub fn parse(text: &str) -> Option<Value> {
    let mut p = Parser {
        b: text.as_bytes(),
        at: 0,
    };
    let v = p.value(0)?;
    p.space();
    (p.at == p.b.len()).then_some(v)
}

struct Parser<'a> {
    b: &'a [u8],
    at: usize,
}

impl Parser<'_> {
    fn space(&mut self) {
        while matches!(self.b.get(self.at), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    fn eat(&mut self, word: &[u8]) -> bool {
        if self.b[self.at..].starts_with(word) {
            self.at += word.len();
            true
        } else {
            false
        }
    }

    fn value(&mut self, depth: usize) -> Option<Value> {
        if depth > MAX_DEPTH {
            return None;
        }
        self.space();
        match *self.b.get(self.at)? {
            b'n' => self.eat(b"null").then_some(Value::Null),
            b't' => self.eat(b"true").then_some(Value::Bool(true)),
            b'f' => self.eat(b"false").then_some(Value::Bool(false)),
            b'"' => self.string().map(Value::Str),
            b'[' => {
                self.at += 1;
                let mut items = Vec::new();
                self.space();
                if self.eat(b"]") {
                    return Some(Value::Array(items));
                }
                loop {
                    items.push(self.value(depth + 1)?);
                    self.space();
                    if self.eat(b"]") {
                        return Some(Value::Array(items));
                    }
                    if !self.eat(b",") {
                        return None;
                    }
                }
            }
            b'{' => {
                self.at += 1;
                let mut members = Vec::new();
                self.space();
                if self.eat(b"}") {
                    return Some(Value::Object(members));
                }
                loop {
                    self.space();
                    if self.b.get(self.at) != Some(&b'"') {
                        return None;
                    }
                    let key = self.string()?;
                    self.space();
                    if !self.eat(b":") {
                        return None;
                    }
                    members.push((key, self.value(depth + 1)?));
                    self.space();
                    if self.eat(b"}") {
                        return Some(Value::Object(members));
                    }
                    if !self.eat(b",") {
                        return None;
                    }
                }
            }
            b'-' | b'0'..=b'9' => self.number(),
            _ => None,
        }
    }

    fn number(&mut self) -> Option<Value> {
        let start = self.at;
        self.eat(b"-");
        let digits = |p: &mut Self| {
            let s = p.at;
            while p.b.get(p.at).is_some_and(u8::is_ascii_digit) {
                p.at += 1;
            }
            p.at - s
        };
        let first = self.b.get(self.at).copied();
        let n = digits(self);
        if n == 0 || (first == Some(b'0') && n > 1) {
            return None;
        }
        let mut float = false;
        if self.eat(b".") {
            float = true;
            if digits(self) == 0 {
                return None;
            }
        }
        if matches!(self.b.get(self.at), Some(b'e' | b'E')) {
            float = true;
            self.at += 1;
            if matches!(self.b.get(self.at), Some(b'+' | b'-')) {
                self.at += 1;
            }
            if digits(self) == 0 {
                return None;
            }
        }
        let text = core::str::from_utf8(&self.b[start..self.at]).ok()?;
        if !float && let Ok(n) = text.parse::<i64>() {
            return Some(Value::Int(n));
        }
        text.parse::<f64>().ok().map(Value::Float)
    }

    fn hex4(&mut self) -> Option<u32> {
        let h = self.b.get(self.at..self.at + 4)?;
        self.at += 4;
        let mut v = 0;
        for c in h {
            v = v * 16 + (*c as char).to_digit(16)?;
        }
        Some(v)
    }

    fn string(&mut self) -> Option<String> {
        self.at += 1; // the quote
        let mut out = String::new();
        loop {
            // A run of plain characters at once: the input is a &str, so whole.
            let start = self.at;
            while let Some(&c) = self.b.get(self.at) {
                if c == b'"' || c == b'\\' || c < 0x20 {
                    break;
                }
                self.at += 1;
            }
            out.push_str(core::str::from_utf8(&self.b[start..self.at]).ok()?);
            match *self.b.get(self.at)? {
                b'"' => {
                    self.at += 1;
                    return Some(out);
                }
                b'\\' => {
                    self.at += 1;
                    let c = *self.b.get(self.at)?;
                    self.at += 1;
                    out.push(match c {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => {
                            let hi = self.hex4()?;
                            let code = if (0xD800..0xDC00).contains(&hi) {
                                if !self.eat(b"\\u") {
                                    return None;
                                }
                                let lo = self.hex4()?;
                                if !(0xDC00..0xE000).contains(&lo) {
                                    return None;
                                }
                                0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00)
                            } else {
                                hi
                            };
                            char::from_u32(code)?
                        }
                        _ => return None,
                    });
                }
                _ => return None, // a control character
            }
        }
    }
}

/// Writes JSON into a string: the caller puts the commas, this the values.
pub struct Writer {
    pub out: String,
}

impl Writer {
    pub fn new() -> Writer {
        Writer { out: String::new() }
    }

    pub fn raw(&mut self, s: &str) -> &mut Self {
        self.out.push_str(s);
        self
    }

    pub fn str(&mut self, s: &str) -> &mut Self {
        self.out.push('"');
        for c in s.chars() {
            match c {
                '"' => self.out.push_str("\\\""),
                '\\' => self.out.push_str("\\\\"),
                '\n' => self.out.push_str("\\n"),
                '\r' => self.out.push_str("\\r"),
                '\t' => self.out.push_str("\\t"),
                c if (c as u32) < 0x20 => {
                    self.out.push_str(&alloc::format!("\\u{:04x}", c as u32));
                }
                c => self.out.push(c),
            }
        }
        self.out.push('"');
        self
    }

    /// `"key":`, after a comma unless `first`.
    pub fn key(&mut self, key: &str, first: bool) -> &mut Self {
        if !first {
            self.out.push(',');
        }
        self.str(key);
        self.out.push(':');
        self
    }

    pub fn int(&mut self, n: i64) -> &mut Self {
        self.out.push_str(&alloc::format!("{n}"));
        self
    }

    pub fn bool(&mut self, b: bool) -> &mut Self {
        self.out.push_str(if b { "true" } else { "false" });
        self
    }
}

impl Default for Writer {
    fn default() -> Writer {
        Writer::new()
    }
}
