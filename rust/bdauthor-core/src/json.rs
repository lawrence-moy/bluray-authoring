//! Lecteur JSON minimal (RFC 8259) sans dependance, pour les fichiers de projet.
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Value>),
    Obj(Vec<(String, Value)>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct JsonError(pub String);
impl fmt::Display for JsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "JSON invalide : {}", self.0) }
}
impl std::error::Error for JsonError {}

impl Value {
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self { Value::Obj(o) => o.iter().find(|(k, _)| k == key).map(|(_, v)| v), _ => None }
    }
    pub fn as_str(&self) -> Option<&str> { if let Value::Str(s) = self { Some(s) } else { None } }
    pub fn as_f64(&self) -> Option<f64> { if let Value::Num(n) = self { Some(*n) } else { None } }
    pub fn as_i64(&self) -> Option<i64> { self.as_f64().filter(|n| n.fract() == 0.0).map(|n| n as i64) }
    pub fn as_bool(&self) -> Option<bool> { if let Value::Bool(b) = self { Some(*b) } else { None } }
    pub fn as_array(&self) -> Option<&[Value]> { if let Value::Arr(a) = self { Some(a) } else { None } }
    pub fn is_null(&self) -> bool { matches!(self, Value::Null) }
}

struct P<'a> { s: &'a [u8], i: usize }

pub fn parse(text: &str) -> Result<Value, JsonError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut p = P { s: text.as_bytes(), i: 0 };
    p.ws();
    let v = p.value(0)?;
    p.ws();
    if p.i != p.s.len() { return p.fail("texte apres la valeur"); }
    Ok(v)
}

impl<'a> P<'a> {
    fn fail<T>(&self, m: &str) -> Result<T, JsonError> {
        let line = self.s[..self.i.min(self.s.len())].iter().filter(|&&b| b == b'\n').count() + 1;
        Err(JsonError(format!("{m} (ligne {line}, octet {})", self.i)))
    }
    fn ws(&mut self) { while self.i < self.s.len() && matches!(self.s[self.i], b' ' | b'\t' | b'\n' | b'\r') { self.i += 1; } }
    fn peek(&self) -> Option<u8> { self.s.get(self.i).copied() }
    fn lit(&mut self, w: &str, v: Value) -> Result<Value, JsonError> {
        if self.s[self.i..].starts_with(w.as_bytes()) { self.i += w.len(); Ok(v) } else { self.fail("mot inconnu") }
    }
    fn value(&mut self, depth: usize) -> Result<Value, JsonError> {
        if depth > 200 { return self.fail("imbrication trop profonde"); }
        match self.peek() {
            None => self.fail("fin inattendue"),
            Some(b'n') => self.lit("null", Value::Null),
            Some(b't') => self.lit("true", Value::Bool(true)),
            Some(b'f') => self.lit("false", Value::Bool(false)),
            Some(b'"') => Ok(Value::Str(self.string()?)),
            Some(b'[') => {
                self.i += 1;
                let mut a = Vec::new();
                self.ws();
                if self.peek() == Some(b']') { self.i += 1; return Ok(Value::Arr(a)); }
                loop {
                    self.ws();
                    a.push(self.value(depth + 1)?);
                    self.ws();
                    match self.peek() {
                        Some(b',') => self.i += 1,
                        Some(b']') => { self.i += 1; return Ok(Value::Arr(a)); }
                        _ => return self.fail("',' ou ']' attendu"),
                    }
                }
            }
            Some(b'{') => {
                self.i += 1;
                let mut o: Vec<(String, Value)> = Vec::new();
                self.ws();
                if self.peek() == Some(b'}') { self.i += 1; return Ok(Value::Obj(o)); }
                loop {
                    self.ws();
                    if self.peek() != Some(b'"') { return self.fail("cle entre guillemets attendue"); }
                    let k = self.string()?;
                    self.ws();
                    if self.peek() != Some(b':') { return self.fail("':' attendu"); }
                    self.i += 1;
                    self.ws();
                    let v = self.value(depth + 1)?;
                    if let Some(e) = o.iter_mut().find(|(kk, _)| *kk == k) { e.1 = v } else { o.push((k, v)) } // la derniere valeur gagne (comme Python)
                    self.ws();
                    match self.peek() {
                        Some(b',') => self.i += 1,
                        Some(b'}') => { self.i += 1; return Ok(Value::Obj(o)); }
                        _ => return self.fail("',' ou '}' attendu"),
                    }
                }
            }
            Some(c) if c == b'-' || c.is_ascii_digit() => {
                let st = self.i;
                while self.i < self.s.len() && matches!(self.s[self.i], b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9') { self.i += 1; }
                std::str::from_utf8(&self.s[st..self.i]).ok().and_then(|t| t.parse::<f64>().ok()).map(Value::Num).ok_or(()).or_else(|_| self.fail("nombre invalide"))
            }
            Some(_) => self.fail("caractere inattendu"),
        }
    }
    fn hex4(&mut self) -> Result<u32, JsonError> {
        if self.i + 4 > self.s.len() { return self.fail("sequence \\u tronquee"); }
        let t = std::str::from_utf8(&self.s[self.i..self.i + 4]).unwrap_or("");
        let v = u32::from_str_radix(t, 16).or_else(|_| self.fail("sequence \\u invalide"))?;
        self.i += 4;
        Ok(v)
    }
    fn string(&mut self) -> Result<String, JsonError> {
        self.i += 1; // guillemet ouvrant
        let mut out: Vec<u8> = Vec::new();
        loop {
            let Some(c) = self.peek() else { return self.fail("chaine non terminee") };
            self.i += 1;
            match c {
                b'"' => return String::from_utf8(out).or_else(|_| self.fail("UTF-8 invalide")),
                b'\\' => {
                    let Some(e) = self.peek() else { return self.fail("echappement tronque") };
                    self.i += 1;
                    let ch = match e {
                        b'"' => '"', b'\\' => '\\', b'/' => '/', b'b' => '\u{8}', b'f' => '\u{c}', b'n' => '\n', b'r' => '\r', b't' => '\t',
                        b'u' => {
                            let mut cp = self.hex4()?;
                            if (0xD800..0xDC00).contains(&cp) && self.s[self.i..].starts_with(b"\\u") {
                                self.i += 2;
                                let lo = self.hex4()?;
                                cp = 0x10000 + ((cp - 0xD800) << 10) + lo.wrapping_sub(0xDC00);
                            }
                            char::from_u32(cp).unwrap_or('\u{fffd}')
                        }
                        _ => return self.fail("echappement inconnu"),
                    };
                    let mut b = [0u8; 4];
                    out.extend_from_slice(ch.encode_utf8(&mut b).as_bytes());
                }
                c if c < 0x20 => return self.fail("caractere de controle dans une chaine"),
                c => out.push(c),
            }
        }
    }
}

// ------------------------------------------------------------------ ecriture
fn esc(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

fn num(n: f64, out: &mut String) {
    if n.fract() == 0.0 && n.abs() < 1e15 { out.push_str(&format!("{}", n as i64)) } else { out.push_str(&format!("{n}")) }
}

impl Value {
    /// JSON indente de 2 espaces, UTF-8 non echappe (lisible et modifiable a la main).
    pub fn to_pretty(&self) -> String {
        let mut s = String::new();
        self.write(&mut s, 0);
        s.push('\n');
        s
    }
    fn write(&self, out: &mut String, depth: usize) {
        let pad = |out: &mut String, d: usize| out.push_str(&"  ".repeat(d));
        match self {
            Value::Null => out.push_str("null"),
            Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Value::Num(n) => num(*n, out),
            Value::Str(s) => esc(s, out),
            Value::Arr(a) if a.is_empty() => out.push_str("[]"),
            Value::Arr(a) => {
                out.push_str("[\n");
                for (i, v) in a.iter().enumerate() {
                    pad(out, depth + 1);
                    v.write(out, depth + 1);
                    out.push_str(if i + 1 < a.len() { ",\n" } else { "\n" });
                }
                pad(out, depth);
                out.push(']');
            }
            Value::Obj(o) if o.is_empty() => out.push_str("{}"),
            Value::Obj(o) => {
                out.push_str("{\n");
                for (i, (k, v)) in o.iter().enumerate() {
                    pad(out, depth + 1);
                    esc(k, out);
                    out.push_str(": ");
                    v.write(out, depth + 1);
                    out.push_str(if i + 1 < o.len() { ",\n" } else { "\n" });
                }
                pad(out, depth);
                out.push('}');
            }
        }
    }
}

/// Differences entre deux objets : (chemin, nouvelle valeur ou `None` si la cle a disparu). Les sous-objets sont compares cle
/// par cle ; les cles `skip` du premier niveau sont ignorees.
pub fn diff(a: &Value, b: &Value, skip: &[&str]) -> Vec<(Vec<String>, Option<Value>)> {
    fn walk(a: &Value, b: &Value, path: &mut Vec<String>, skip: &[&str], out: &mut Vec<(Vec<String>, Option<Value>)>) {
        let (Value::Obj(oa), Value::Obj(ob)) = (a, b) else { return };
        let mut seen = std::collections::HashSet::new();
        for k in oa.iter().map(|e| &e.0).chain(ob.iter().map(|e| &e.0)) {
            if !seen.insert(k.clone()) || (path.is_empty() && skip.contains(&k.as_str())) { continue; }
            let (va, vb) = (a.get(k), b.get(k));
            if va == vb { continue; }
            path.push(k.clone());
            match (va, vb) {
                (Some(x @ Value::Obj(_)), Some(y @ Value::Obj(_))) => walk(x, y, path, skip, out),
                _ => out.push((path.clone(), vb.cloned())),
            }
            path.pop();
        }
    }
    let mut out = Vec::new();
    walk(a, b, &mut Vec::new(), skip, &mut out);
    out
}

/// Ecrit (ou retire, `None`) la valeur au chemin `path`, en creant les objets intermediaires.
pub fn set_path(v: &mut Value, path: &[String], val: &Option<Value>) {
    let Value::Obj(o) = v else { return };
    let Some((k, rest)) = path.split_first() else { return };
    let pos = o.iter().position(|e| &e.0 == k);
    if rest.is_empty() {
        match (pos, val) {
            (Some(p), Some(x)) => o[p].1 = x.clone(),
            (None, Some(x)) => o.push((k.clone(), x.clone())),
            (Some(p), None) => { o.remove(p); }
            (None, None) => {}
        }
        return;
    }
    if val.is_none() && pos.is_none() { return; }
    let p = match pos {
        Some(p) => p,
        None => { o.push((k.clone(), Value::Obj(Vec::new()))); o.len() - 1 }
    };
    if !matches!(o[p].1, Value::Obj(_)) { o[p].1 = Value::Obj(Vec::new()); }
    set_path(&mut o[p].1, rest, val);
}
