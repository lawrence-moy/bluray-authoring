//! Ecriture PNG sans dependance (deflate « stocke », non compresse) : sert aux fonds generes passes a ffmpeg.
use crate::palette::RgbaImage;

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &x in data {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let c = crc32(&out[start..]);
    out.extend_from_slice(&c.to_be_bytes());
}

pub fn write_png(im: &RgbaImage) -> Vec<u8> {
    let stride = im.width as usize * 4;
    let mut raw = Vec::with_capacity((stride + 1) * im.height as usize);
    for y in 0..im.height as usize {
        raw.push(0); // filtre « aucun »
        raw.extend_from_slice(&im.data[y * stride..(y + 1) * stride]);
    }
    let mut z = vec![0x78, 0x01];
    let mut chunks = raw.chunks(65535).peekable();
    if chunks.peek().is_none() {
        z.extend_from_slice(&[1, 0, 0, 0xFF, 0xFF]);
    }
    while let Some(c) = chunks.next() {
        z.push(if chunks.peek().is_none() { 1 } else { 0 });
        z.extend_from_slice(&(c.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(c.len() as u16)).to_le_bytes());
        z.extend_from_slice(c);
    }
    z.extend_from_slice(&adler32(&raw).to_be_bytes());
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&im.width.to_be_bytes());
    ihdr.extend_from_slice(&im.height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

/// PNG compresse pour l'impression : RVB si l'image est opaque (RVBA sinon), avec la resolution `dpi` enregistree (chunk pHYs).
pub fn write_png_print(im: &RgbaImage, dpi: u32) -> Vec<u8> {
    let opaque = im.data.chunks_exact(4).all(|p| p[3] == 255);
    let (bpp, ctype) = if opaque { (3usize, 2u8) } else { (4usize, 6u8) };
    let mut raw = Vec::with_capacity((im.width as usize * bpp + 1) * im.height as usize);
    let stride = im.width as usize * 4;
    for y in 0..im.height as usize {
        raw.push(0);
        let row = &im.data[y * stride..(y + 1) * stride];
        if opaque { for p in row.chunks_exact(4) { raw.extend_from_slice(&p[..3]); } } else { raw.extend_from_slice(row); }
    }
    let z = crate::deflate::zlib_compress(&raw);
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&im.width.to_be_bytes());
    ihdr.extend_from_slice(&im.height.to_be_bytes());
    ihdr.extend_from_slice(&[8, ctype, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    let ppm = (dpi as f64 / 0.0254).round() as u32;
    let mut phys = Vec::new();
    phys.extend_from_slice(&ppm.to_be_bytes());
    phys.extend_from_slice(&ppm.to_be_bytes());
    phys.push(1);
    chunk(&mut out, b"pHYs", &phys);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}
