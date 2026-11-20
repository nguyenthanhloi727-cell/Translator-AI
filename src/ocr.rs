//! OCR tieng Anh bang Windows.Media.Ocr (co san trong Windows, chay CPU, khong tai model).

use crate::frame::Frame;
use windows::Globalization::Language;
use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Storage::Streams::DataWriter;
use windows::core::HSTRING;

/// Dong chu OCR doc duoc, toa do tinh theo anh goc (truoc khi phong to).
#[derive(Clone, Debug)]
pub struct OcrLine {
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

pub struct Ocr {
    engine: OcrEngine,
    max_dim: u32,
}

const MISSING_LANG: &str = "may thieu goi ngon ngu OCR tieng Anh. Vao Settings, Time & language, \
Language & region, them English (US) va chon cai dat 'Optical character recognition'";

impl Ocr {
    pub fn new() -> Result<Self, String> {
        let lang = Language::CreateLanguage(&HSTRING::from("en-US")).map_err(|e| e.to_string())?;
        if !OcrEngine::IsLanguageSupported(&lang).unwrap_or(false) {
            return Err(MISSING_LANG.into());
        }
        let engine = OcrEngine::TryCreateFromLanguage(&lang).map_err(|_| MISSING_LANG.to_string())?;
        let max_dim = OcrEngine::MaxImageDimension().map_err(|e| e.to_string())?;
        Ok(Self { engine, max_dim })
    }

    /// OCR mot khung. Chu nho thi phong to truoc (xem `scale_for`).
    pub fn recognize(&self, frame: &Frame) -> Result<Vec<OcrLine>, String> {
        let mut k = scale_for(frame.height);
        while k > 1 && (frame.width * k > self.max_dim || frame.height * k > self.max_dim) {
            k -= 1;
        }
        let scaled;
        let src = if k > 1 {
            scaled = upscale(frame, k);
            &scaled
        } else {
            frame
        };
        if src.width > self.max_dim || src.height > self.max_dim {
            return Err(format!("anh qua lon cho OCR (toi da {} px moi canh)", self.max_dim));
        }

        let bmp = to_bitmap(src).map_err(|e| e.to_string())?;
        let res = self
            .engine
            .RecognizeAsync(&bmp)
            .and_then(|op| op.join())
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for line in res.Lines().map_err(|e| e.to_string())? {
            let text = line.Text().map_err(|e| e.to_string())?.to_string();
            let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, 0f32, 0f32);
            for word in line.Words().map_err(|e| e.to_string())? {
                let r = word.BoundingRect().map_err(|e| e.to_string())?;
                x0 = x0.min(r.X);
                y0 = y0.min(r.Y);
                x1 = x1.max(r.X + r.Width);
                y1 = y1.max(r.Y + r.Height);
            }
            if x0 > x1 {
                continue;
            }
            let k = k as f32;
            out.push(OcrLine { text, x: x0 / k, y: y0 / k, w: (x1 - x0) / k, h: (y1 - y0) / k });
        }
        Ok(out)
    }
}

/// He so phong to theo chieu cao vung. Windows OCR doc tot khi chu cao khoang 30 px tro len.
pub fn scale_for(height: u32) -> u32 {
    match height {
        0..=60 => 3,
        61..=120 => 2,
        _ => 1,
    }
}

/// Phong to nguyen he so k bang noi suy tuyen tinh theo chieu ngang va doc.
pub fn upscale(f: &Frame, k: u32) -> Frame {
    let (w, h) = (f.width as usize, f.height as usize);
    let (nw, nh) = (w * k as usize, h * k as usize);
    let mut out = vec![0u8; nw * nh * 4];
    let kf = k as f32;
    for y in 0..nh {
        let fy = ((y as f32 + 0.5) / kf - 0.5).clamp(0.0, (h - 1) as f32);
        let y0 = fy as usize;
        let y1 = (y0 + 1).min(h - 1);
        let ty = fy - y0 as f32;
        for x in 0..nw {
            let fx = ((x as f32 + 0.5) / kf - 0.5).clamp(0.0, (w - 1) as f32);
            let x0 = fx as usize;
            let x1 = (x0 + 1).min(w - 1);
            let tx = fx - x0 as f32;
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

fn to_bitmap(f: &Frame) -> windows::core::Result<SoftwareBitmap> {
    let w = DataWriter::new()?;
    w.WriteBytes(&f.bgra)?;
    let buf = w.DetachBuffer()?;
    SoftwareBitmap::CreateCopyFromBuffer(&buf, BitmapPixelFormat::Bgra8, f.width as i32, f.height as i32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Foundation::COLORREF;
    use windows::Win32::Graphics::Gdi::*;
    use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize};
    use windows::core::w;

    /// Ve chu bang GDI vao anh BGRA (chu `fg`, nen `bg`, dang 0xRRGGBB).
    fn render(text: &str, px: i32, fg: u32, bg: u32, noise: bool) -> Frame {
        let width = (text.len() as i32 * px).max(64) + 40;
        let height = px * 2 + 20;
        unsafe {
            let dc = CreateCompatibleDC(None);
            let bi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width,
                    biHeight: -height,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits = std::ptr::null_mut();
            let bmp = CreateDIBSection(Some(dc), &bi, DIB_RGB_COLORS, &mut bits, None, 0).unwrap();
            let old = SelectObject(dc, bmp.into());
            let font = CreateFontW(
                -px, 0, 0, 0, 700, 0, 0, 0, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS,
                ANTIALIASED_QUALITY, 0, w!("Arial"),
            );
            let oldf = SelectObject(dc, font.into());
            let n = (width * height) as usize;
            let buf = std::slice::from_raw_parts_mut(bits as *mut u32, n);
            // GDI xep mau la 0x00RRGGBB trong u32 cua DIB 32 bit
            for (i, p) in buf.iter_mut().enumerate() {
                *p = if noise {
                    // nen "game": doc theo chieu ngang, them nhieu gia lap
                    let x = (i as u32 % width as u32) as u8;
                    let nz = ((i as u32).wrapping_mul(2654435761) >> 28) as u8;
                    let v = 30 + x / 8 + nz;
                    (v as u32) << 16 | (v as u32) << 8 | v as u32
                } else {
                    bg
                };
            }
            SetBkMode(dc, TRANSPARENT);
            // COLORREF la 0x00BBGGRR
            let fgref = (fg & 0xff) << 16 | (fg & 0xff00) | (fg >> 16);
            SetTextColor(dc, COLORREF(fgref));
            let wide: Vec<u16> = text.encode_utf16().collect();
            let _ = TextOutW(dc, 20, 10, &wide);
            let _ = GdiFlush();
            let mut bgra = Vec::with_capacity(n * 4);
            for &p in buf.iter() {
                bgra.extend_from_slice(&[p as u8, (p >> 8) as u8, (p >> 16) as u8, 255]);
            }
            SelectObject(dc, oldf);
            SelectObject(dc, old);
            let _ = DeleteObject(font.into());
            let _ = DeleteObject(bmp.into());
            let _ = DeleteDC(dc);
            Frame { width: width as u32, height: height as u32, bgra }
        }
    }

    fn norm(s: &str) -> String {
        s.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase()
    }

    fn ocr() -> Ocr {
        let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
        Ocr::new().expect("khoi tao OCR")
    }

    fn read_all(o: &Ocr, f: &Frame) -> String {
        o.recognize(f).unwrap().iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join(" ")
    }

    #[test]
    fn reads_dark_on_light_many_sizes() {
        let o = ocr();
        for px in [16, 20, 24, 32, 48] {
            let f = render("Buy a new outfit", px, 0x000000, 0xFFFFFF, false);
            let got = read_all(&o, &f);
            assert_eq!(norm(&got), "buyanewoutfit", "co chu {px}px doc ra '{got}'");
        }
    }

    #[test]
    fn reads_white_on_noisy_dark_background() {
        let o = ocr();
        for px in [24, 32, 40] {
            let f = render("Follow Franklin to the car", px, 0xFFFFFF, 0, true);
            let got = read_all(&o, &f);
            assert_eq!(norm(&got), "followfranklintothecar", "co chu {px}px doc ra '{got}'");
        }
    }

    #[test]
    fn line_position_is_in_original_coordinates() {
        let o = ocr();
        let f = render("Los Santos", 24, 0x000000, 0xFFFFFF, false);
        let lines = o.recognize(&f).unwrap();
        assert_eq!(lines.len(), 1);
        let l = &lines[0];
        assert!(l.x >= 10.0 && l.x < 40.0, "x={}", l.x);
        assert!(l.y >= 0.0 && l.y + l.h <= f.height as f32 + 1.0, "y={} h={}", l.y, l.h);
        assert!(l.w > 60.0 && l.w < f.width as f32, "w={}", l.w);
    }

    /// Do toc do: `cargo test --release bench_speed -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn bench_speed() {
        let o = ocr();
        let text = "I told you not to come back here, Michael";
        for (px, noise) in [(24, false), (32, true), (40, true)] {
            let f = render(text, px, 0xFFFFFF, 0x202020, noise);
            o.recognize(&f).unwrap();
            let mut ms: Vec<f64> = (0..30)
                .map(|_| {
                    let t = std::time::Instant::now();
                    o.recognize(&f).unwrap();
                    t.elapsed().as_secs_f64() * 1000.0
                })
                .collect();
            ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
            println!("chu {px}px, anh {}x{}: trung vi {:.1} ms, max {:.1} ms, doc: '{}'", f.width, f.height, ms[15], ms[29], read_all(&o, &f));
        }
    }

    #[test]
    fn empty_image_gives_no_lines() {
        let o = ocr();
        let f = Frame { width: 200, height: 40, bgra: vec![255; 200 * 40 * 4] };
        assert!(o.recognize(&f).unwrap().is_empty());
    }

    #[test]
    fn upscale_keeps_size_and_flat_color() {
        let f = Frame { width: 4, height: 3, bgra: vec![200; 4 * 3 * 4] };
        let u = upscale(&f, 3);
        assert_eq!((u.width, u.height), (12, 9));
        assert!(u.bgra.iter().all(|&b| b == 200));
    }

    #[test]
    fn scale_for_small_text_is_bigger() {
        assert_eq!(scale_for(40), 3);
        assert_eq!(scale_for(100), 2);
        assert_eq!(scale_for(300), 1);
    }
}
