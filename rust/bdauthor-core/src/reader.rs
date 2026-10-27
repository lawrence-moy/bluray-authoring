//! Lecture/ecriture binaire big-endian (equivalent de `reader.py`).
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub struct ParseError(pub String);
impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::error::Error for ParseError {}
pub type PResult<T> = std::result::Result<T, ParseError>;

pub fn err<T>(m: impl Into<String>) -> PResult<T> {
    Err(ParseError(m.into()))
}

pub struct Reader<'a> {
    pub data: &'a [u8],
    pub pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Reader { data, pos: 0 }
    }
    pub fn raw(&mut self, n: usize) -> PResult<&'a [u8]> {
        match self.pos.checked_add(n) {
            Some(end) if end <= self.data.len() => {
                let b = &self.data[self.pos..end];
                self.pos = end;
                Ok(b)
            }
            _ => err(format!("lecture de {n} octets impossible a l'offset {}", self.pos)),
        }
    }
    pub fn seek(&mut self, pos: usize) {
        self.pos = pos;
    }
    pub fn u8(&mut self) -> PResult<u8> {
        Ok(self.raw(1)?[0])
    }
    pub fn u16(&mut self) -> PResult<u16> {
        Ok(u16::from_be_bytes(self.raw(2)?.try_into().unwrap()))
    }
    pub fn u32(&mut self) -> PResult<u32> {
        Ok(u32::from_be_bytes(self.raw(4)?.try_into().unwrap()))
    }
    /// Texte ASCII de `n` octets.
    pub fn ascii(&mut self, n: usize) -> PResult<String> {
        Ok(self.raw(n)?.iter().map(|&b| b as char).collect())
    }
}

/// Extrait `width` bits de `value` a partir du bit `shift` (0 = poids faible).
pub fn bits(value: u32, width: u32, shift: u32) -> u32 {
    ((value as u64 >> shift) & ((1u64 << width) - 1)) as u32
}

/// Ecriture big-endian dans un `Vec<u8>`.
pub trait Put {
    fn p8(&mut self, v: u8);
    fn p16(&mut self, v: u16);
    fn p32(&mut self, v: u32);
    fn zeros(&mut self, n: usize);
    fn text(&mut self, s: &str);
}
impl Put for Vec<u8> {
    fn p8(&mut self, v: u8) { self.push(v) }
    fn p16(&mut self, v: u16) { self.extend_from_slice(&v.to_be_bytes()) }
    fn p32(&mut self, v: u32) { self.extend_from_slice(&v.to_be_bytes()) }
    fn zeros(&mut self, n: usize) { self.resize(self.len() + n, 0) }
    fn text(&mut self, s: &str) { self.extend(s.chars().map(|c| c as u32 as u8)) }
}
