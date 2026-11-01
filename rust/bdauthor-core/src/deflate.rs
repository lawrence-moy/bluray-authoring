//! Compression zlib sans dependance : LZ77 (chaines de hachage) + codes de Huffman fixes. Utilisee pour les images des PDF.

const LEN_BASE: [u16; 29] = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
const LEN_EXTRA: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DIST_BASE: [u16; 30] = [1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577];
const DIST_EXTRA: [u8; 30] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];

struct Bits {
    out: Vec<u8>,
    acc: u32,
    n: u32,
}

impl Bits {
    /// `n` bits de `v`, poids faible d'abord
    fn put(&mut self, v: u32, n: u32) {
        self.acc |= v << self.n;
        self.n += n;
        while self.n >= 8 {
            self.out.push(self.acc as u8);
            self.acc >>= 8;
            self.n -= 8;
        }
    }
    /// code de Huffman de `n` bits : transmis poids fort d'abord
    fn code(&mut self, c: u32, n: u32) {
        let mut r = 0;
        for i in 0..n {
            if c & (1 << i) != 0 { r |= 1 << (n - 1 - i); }
        }
        self.put(r, n);
    }
    fn lit(&mut self, s: u32) {
        match s {
            0..=143 => self.code(0x30 + s, 8),
            144..=255 => self.code(0x190 + s - 144, 9),
            256..=279 => self.code(s - 256, 7),
            _ => self.code(0xC0 + s - 280, 8),
        }
    }
    fn finish(mut self) -> Vec<u8> {
        if self.n > 0 { self.out.push(self.acc as u8); }
        self.out
    }
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in data.chunks(5552) {
        for &x in chunk {
            a += x as u32;
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    (b << 16) | a
}

/// Flux zlib (RFC 1950) contenant `data`.
pub fn zlib_compress(data: &[u8]) -> Vec<u8> {
    const WIN: usize = 32768;
    const HASH: usize = 1 << 15;
    const MAX_CHAIN: usize = 24;
    let mut w = Bits { out: vec![0x78, 0x01], acc: 0, n: 0 };
    w.put(1, 1); // dernier bloc
    w.put(1, 2); // codes fixes
    let n = data.len();
    let mut head = vec![u32::MAX; HASH];
    let mut prev = vec![u32::MAX; WIN];
    let hash = |i: usize| -> usize { (((data[i] as usize) << 10) ^ ((data[i + 1] as usize) << 5) ^ data[i + 2] as usize) & (HASH - 1) };
    let insert = |i: usize, head: &mut Vec<u32>, prev: &mut Vec<u32>| {
        if i + 2 < n {
            let h = hash(i);
            prev[i & (WIN - 1)] = head[h];
            head[h] = i as u32;
        }
    };
    let mut i = 0;
    while i < n {
        let (mut best_len, mut best_dist) = (0usize, 0usize);
        if i + 2 < n {
            let mut cand = head[hash(i)];
            let mut chain = 0;
            let max_len = (n - i).min(258);
            while cand != u32::MAX && chain < MAX_CHAIN {
                let c = cand as usize;
                if i - c > WIN - 1 || c >= i { break; }
                let mut l = 0;
                while l < max_len && data[c + l] == data[i + l] { l += 1; }
                if l > best_len {
                    best_len = l;
                    best_dist = i - c;
                    if l == max_len { break; }
                }
                let nx = prev[c & (WIN - 1)];
                if nx != u32::MAX && nx as usize >= c { break; }
                cand = nx;
                chain += 1;
            }
        }
        if best_len >= 3 {
            let li = LEN_BASE.iter().rposition(|&b| b as usize <= best_len).unwrap();
            w.lit(257 + li as u32);
            w.put((best_len - LEN_BASE[li] as usize) as u32, LEN_EXTRA[li] as u32);
            let di = DIST_BASE.iter().rposition(|&b| b as usize <= best_dist).unwrap();
            w.code(di as u32, 5);
            w.put((best_dist - DIST_BASE[di] as usize) as u32, DIST_EXTRA[di] as u32);
            for k in 0..best_len { insert(i + k, &mut head, &mut prev); }
            i += best_len;
        } else {
            w.lit(data[i] as u32);
            insert(i, &mut head, &mut prev);
            i += 1;
        }
    }
    w.lit(256);
    let mut out = w.finish();
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

/// Decompression des flux produits par `zlib_compress` (codes fixes uniquement) : sert aux tests et a la relecture.
pub fn zlib_decompress_fixed(z: &[u8]) -> Result<Vec<u8>, String> {
    if z.len() < 6 || z[0] != 0x78 { return Err("en-tete zlib invalide".into()); }
    let d = &z[2..z.len() - 4];
    let mut pos = 0usize;
    let bit = |n: u32, pos: &mut usize| -> Result<u32, String> {
        let mut v = 0;
        for k in 0..n {
            let byte = d.get(*pos / 8).ok_or("flux tronque")?;
            v |= (((byte >> (*pos % 8)) & 1) as u32) << k;
            *pos += 1;
        }
        Ok(v)
    };
    if bit(1, &mut pos)? != 1 || bit(2, &mut pos)? != 1 { return Err("seuls les blocs fixes sont geres".into()); }
    let mut out: Vec<u8> = Vec::new();
    loop {
        // symbole : lecture bit par bit, poids fort d'abord
        let mut code = 0u32;
        let mut len = 0u32;
        let sym = loop {
            code = (code << 1) | bit(1, &mut pos)?;
            len += 1;
            match len {
                7 if code <= 0x17 => break 256 + code,
                8 if (0x30..=0xBF).contains(&code) => break code - 0x30,
                8 if (0xC0..=0xC7).contains(&code) => break 280 + code - 0xC0,
                9 if (0x190..=0x1FF).contains(&code) => break 144 + code - 0x190,
                9.. => return Err("code invalide".into()),
                _ => {}
            }
        };
        if sym < 256 { out.push(sym as u8); continue; }
        if sym == 256 { break; }
        let li = (sym - 257) as usize;
        let l = LEN_BASE[li] as usize + bit(LEN_EXTRA[li] as u32, &mut pos)? as usize;
        let mut dc = 0u32;
        for _ in 0..5 { dc = (dc << 1) | bit(1, &mut pos)?; }
        let dist = DIST_BASE[dc as usize] as usize + bit(DIST_EXTRA[dc as usize] as u32, &mut pos)? as usize;
        if dist > out.len() { return Err("distance invalide".into()); }
        for _ in 0..l { let b = out[out.len() - dist]; out.push(b); }
    }
    Ok(out)
}
