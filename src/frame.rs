//! Pure helpers for captured frames: region parsing, change detection hash, BMP encoding.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Region {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl Region {
    /// Parse "x,y,w,h" (pixels in the captured image).
    pub fn parse(s: &str) -> Result<Self, String> {
        let parts: Vec<&str> = s.split(',').map(str::trim).collect();
        if parts.len() != 4 {
            return Err("region phai co dang x,y,w,h".into());
        }
        let n: Result<Vec<u32>, _> = parts.iter().map(|p| p.parse::<u32>()).collect();
        let n = n.map_err(|_| "region chi gom so nguyen khong am".to_string())?;
        if n[2] == 0 || n[3] == 0 {
            return Err("region phai co w va h lon hon 0".into());
        }
        Ok(Self { x: n[0], y: n[1], w: n[2], h: n[3] })
    }

    /// Clamp to an image of `width` x `height`. None if nothing is left.
    pub fn clamp(self, width: u32, height: u32) -> Option<Self> {
        if self.x >= width || self.y >= height {
            return None;
        }
        Some(Self {
            x: self.x,
            y: self.y,
            w: self.w.min(width - self.x),
            h: self.h.min(height - self.y),
        })
    }
}

/// A tightly packed BGRA8 image.
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

/// FNV-1a, enough to tell "did the subtitle area change".
pub fn fnv1a(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// Encode a BGRA frame as a 24-bit BMP (no extra dependencies).
pub fn encode_bmp(f: &Frame) -> Vec<u8> {
    let row = (f.width as usize * 3).div_ceil(4) * 4;
    let size = 54 + row * f.height as usize;
    let mut out = Vec::with_capacity(size);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&(size as u32).to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&54u32.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(f.width as i32).to_le_bytes());
    out.extend_from_slice(&(f.height as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&24u16.to_le_bytes());
    out.extend_from_slice(&[0; 24]);
    let pad = row - f.width as usize * 3;
    for y in (0..f.height as usize).rev() {
        let line = &f.bgra[y * f.width as usize * 4..(y + 1) * f.width as usize * 4];
        for px in line.as_chunks::<4>().0 {
            out.extend_from_slice(&px[..3]);
        }
        out.extend(std::iter::repeat_n(0u8, pad));
    }
    out
}

/// Decode an uncompressed 24 or 32-bit BMP (what `encode_bmp` writes, plus common screenshots).
pub fn decode_bmp(d: &[u8]) -> Result<Frame, String> {
    let bad = || "khong phai file BMP 24/32-bit khong nen".to_string();
    if d.len() < 54 || &d[..2] != b"BM" {
        return Err(bad());
    }
    let u32_at = |o: usize| u32::from_le_bytes(d[o..o + 4].try_into().unwrap());
    let off = u32_at(10) as usize;
    let (w, h) = (u32_at(18) as i32, u32_at(22) as i32);
    let bpp = u16::from_le_bytes([d[28], d[29]]);
    if u32_at(30) != 0 && u32_at(30) != 3 || !(bpp == 24 || bpp == 32) || w <= 0 || h == 0 {
        return Err(bad());
    }
    let (w, top_down, hh) = (w as usize, h < 0, h.unsigned_abs() as usize);
    let bytes = bpp as usize / 8;
    let row = (w * bytes).div_ceil(4) * 4;
    if d.len() < off + row * hh {
        return Err("file BMP bi cat cut".into());
    }
    let mut bgra = Vec::with_capacity(w * hh * 4);
    for y in 0..hh {
        let sy = if top_down { y } else { hh - 1 - y };
        let line = &d[off + sy * row..][..w * bytes];
        for px in line.chunks_exact(bytes) {
            bgra.extend_from_slice(&[px[0], px[1], px[2], 255]);
        }
    }
    Ok(Frame { width: w as u32, height: hh as u32, bgra })
}

/// Enlarge by an integer factor with bilinear filtering (helps OCR on small text).
pub fn upscale(f: &Frame, k: u32) -> Frame {
    let (w, h, k) = (f.width as usize, f.height as usize, k as usize);
    let (nw, nh) = (w * k, h * k);
    let mut out = vec![0u8; nw * nh * 4];
    for y in 0..nh {
        let fy = ((y as f32 + 0.5) / k as f32 - 0.5).clamp(0.0, (h - 1) as f32);
        let (y0, ty) = (fy as usize, fy.fract());
        let y1 = (y0 + 1).min(h - 1);
        for x in 0..nw {
            let fx = ((x as f32 + 0.5) / k as f32 - 0.5).clamp(0.0, (w - 1) as f32);
            let (x0, tx) = (fx as usize, fx.fract());
            let x1 = (x0 + 1).min(w - 1);
            for c in 0..4 {
                let p = |xx: usize, yy: usize| f.bgra[(yy * w + xx) * 4 + c] as f32;
                let top = p(x0, y0) * (1.0 - tx) + p(x1, y0) * tx;
                let bot = p(x0, y1) * (1.0 - tx) + p(x1, y1) * tx;
                out[(y * nw + x) * 4 + c] = (top * (1.0 - ty) + bot * ty + 0.5) as u8;
            }
        }
    }
    Frame { width: nw as u32, height: nh as u32, bgra: out }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bmp_roundtrip() {
        let bgra: Vec<u8> = (0..5 * 3).flat_map(|i| [i as u8, 2 * i as u8, 3 * i as u8, 255]).collect();
        let f = Frame { width: 5, height: 3, bgra: bgra.clone() };
        let g = decode_bmp(&encode_bmp(&f)).unwrap();
        assert_eq!((g.width, g.height), (5, 3));
        assert_eq!(g.bgra, bgra);
        assert!(decode_bmp(b"nope").is_err());
    }

    #[test]
    fn upscale_keeps_flat_color_and_size() {
        let f = Frame { width: 2, height: 2, bgra: vec![100; 16] };
        let g = upscale(&f, 3);
        assert_eq!((g.width, g.height), (6, 6));
        assert!(g.bgra.iter().all(|&b| b == 100));
    }

    #[test]
    fn parse_region_ok() {
        assert_eq!(
            Region::parse("10, 20,300,40").unwrap(),
            Region { x: 10, y: 20, w: 300, h: 40 }
        );
    }

    #[test]
    fn parse_region_rejects_bad_input() {
        assert!(Region::parse("1,2,3").is_err());
        assert!(Region::parse("a,b,c,d").is_err());
        assert!(Region::parse("-1,0,5,5").is_err());
        assert!(Region::parse("0,0,0,5").is_err());
    }

    #[test]
    fn clamp_cuts_to_image() {
        let r = Region { x: 90, y: 90, w: 50, h: 50 };
        assert_eq!(r.clamp(100, 100), Some(Region { x: 90, y: 90, w: 10, h: 10 }));
        assert_eq!(r.clamp(80, 100), None);
    }

    #[test]
    fn hash_detects_change() {
        let a = vec![1u8, 2, 3, 4];
        let mut b = a.clone();
        assert_eq!(fnv1a(&a), fnv1a(&b));
        b[2] = 9;
        assert_ne!(fnv1a(&a), fnv1a(&b));
    }

    #[test]
    fn bmp_has_valid_header_and_size() {
        let f = Frame { width: 3, height: 2, bgra: vec![7; 3 * 2 * 4] };
        let bmp = encode_bmp(&f);
        assert_eq!(&bmp[..2], b"BM");
        // 3 px * 3 bytes = 9 -> padded to 12 per row
        assert_eq!(bmp.len(), 54 + 12 * 2);
        assert_eq!(u32::from_le_bytes(bmp[2..6].try_into().unwrap()) as usize, bmp.len());
    }
}
