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

/// Decode an uncompressed 24-bit or 32-bit BMP into a BGRA frame.
pub fn decode_bmp(data: &[u8]) -> Result<Frame, String> {
    let u32_at = |o: usize| data.get(o..o + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()));
    if data.get(..2) != Some(b"BM") {
        return Err("khong phai file BMP".into());
    }
    let off = u32_at(10).ok_or("BMP bi cut")? as usize;
    let width = u32_at(18).ok_or("BMP bi cut")? as i32;
    let height = u32_at(22).ok_or("BMP bi cut")? as i32;
    let bpp = data.get(28..30).map(|b| u16::from_le_bytes([b[0], b[1]])).ok_or("BMP bi cut")?;
    let compression = u32_at(30).ok_or("BMP bi cut")?;
    if compression != 0 || (bpp != 24 && bpp != 32) || width <= 0 || height == 0 {
        return Err("chi ho tro BMP 24/32-bit khong nen".into());
    }
    let (w, h) = (width as usize, height.unsigned_abs() as usize);
    let bottom_up = height > 0;
    let bytes = bpp as usize / 8;
    let row = (w * bytes).div_ceil(4) * 4;
    if data.len() < off + row * h {
        return Err("BMP bi cut".into());
    }
    let mut bgra = vec![255u8; w * h * 4];
    for y in 0..h {
        let src_y = if bottom_up { h - 1 - y } else { y };
        let line = &data[off + src_y * row..off + src_y * row + w * bytes];
        for (x, px) in line.chunks_exact(bytes).enumerate() {
            let d = (y * w + x) * 4;
            bgra[d..d + 3].copy_from_slice(&px[..3]);
        }
    }
    Ok(Frame { width: w as u32, height: h as u32, bgra })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bmp_roundtrip() {
        let mut bgra = vec![255u8; 5 * 3 * 4];
        for (i, b) in bgra.iter_mut().enumerate() {
            if i % 4 != 3 {
                *b = (i * 7) as u8;
            }
        }
        let f = Frame { width: 5, height: 3, bgra: bgra.clone() };
        let d = decode_bmp(&encode_bmp(&f)).unwrap();
        assert_eq!((d.width, d.height), (5, 3));
        assert_eq!(d.bgra, bgra);
        assert!(decode_bmp(b"nope").is_err());
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
