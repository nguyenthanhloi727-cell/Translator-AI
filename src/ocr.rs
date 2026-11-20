//! English OCR through Windows.Media.Ocr (built in, runs on CPU, no model download).

use crate::frame::Frame;
use windows::Globalization::Language;
use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Storage::Streams::DataWriter;
use windows::core::HSTRING;

/// One recognized line with its bounding box in the coordinates of the input frame.
#[derive(Clone, Debug, PartialEq)]
pub struct OcrLine {
    pub text: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Clone, Debug, Default)]
pub struct OcrOutput {
    pub lines: Vec<OcrLine>,
}

impl OcrOutput {
    #[cfg(test)]
    pub fn text(&self) -> String {
        self.lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join("
")
    }
}

/// Subtitle strips at most this tall are enlarged before OCR (small text reads better).
const UPSCALE_MAX_HEIGHT: u32 = 160;
const UPSCALE: u32 = 2;

/// Bilinear enlarge by an integer factor.
fn upscale(f: &Frame, k: u32) -> Frame {
    let (w, h) = (f.width as usize, f.height as usize);
    let (nw, nh) = (w * k as usize, h * k as usize);
    let mut bgra = vec![0u8; nw * nh * 4];
    let kf = k as f32;
    for y in 0..nh {
        let sy = ((y as f32 + 0.5) / kf - 0.5).clamp(0.0, (h - 1) as f32);
        let (y0, fy) = (sy as usize, sy.fract());
        let y1 = (y0 + 1).min(h - 1);
        for x in 0..nw {
            let sx = ((x as f32 + 0.5) / kf - 0.5).clamp(0.0, (w - 1) as f32);
            let (x0, fx) = (sx as usize, sx.fract());
            let x1 = (x0 + 1).min(w - 1);
            for c in 0..4 {
                let p = |xx: usize, yy: usize| f.bgra[(yy * w + xx) * 4 + c] as f32;
                let top = p(x0, y0) * (1.0 - fx) + p(x1, y0) * fx;
                let bot = p(x0, y1) * (1.0 - fx) + p(x1, y1) * fx;
                bgra[(y * nw + x) * 4 + c] = (top * (1.0 - fy) + bot * fy + 0.5) as u8;
            }
        }
    }
    Frame { width: nw as u32, height: nh as u32, bgra }
}

pub struct Ocr {
    engine: OcrEngine,
}

impl Ocr {
    /// Create an English engine. Fails with a clear message when the language pack is missing.
    pub fn new() -> Result<Self, String> {
        let lang = Language::CreateLanguage(&HSTRING::from("en-US")).map_err(|e| e.to_string())?;
        let engine = if OcrEngine::IsLanguageSupported(&lang).unwrap_or(false) {
            OcrEngine::TryCreateFromLanguage(&lang).ok()
        } else {
            None
        };
        let engine = engine.ok_or(
            "may thieu goi ngon ngu OCR tieng Anh. Cai bang: Settings > Time & language > \
             Language & region > English (United States) > Language options > cai goi \
             'Optical character recognition' (hoac PowerShell admin: \
             Add-WindowsCapability -Online -Name Language.OCR~~~en-US~0.0.1.0)",
        )?;
        Ok(Self { engine })
    }

    /// Largest width/height the engine accepts.
    pub fn max_dimension() -> u32 {
        OcrEngine::MaxImageDimension().unwrap_or(2600)
    }

    /// OCR a frame; small subtitle strips are enlarged first, boxes stay in input coordinates.
    pub fn recognize(&self, f: &Frame) -> Result<OcrOutput, String> {
        let k = if f.height > 0 && f.height <= UPSCALE_MAX_HEIGHT { UPSCALE } else { 1 };
        if k == 1 || f.width * k > Self::max_dimension() {
            return self.recognize_raw(f, 1.0);
        }
        self.recognize_raw(&upscale(f, k), k as f64)
    }

    fn recognize_raw(&self, f: &Frame, scale: f64) -> Result<OcrOutput, String> {
        let max = Self::max_dimension();
        if f.width == 0 || f.height == 0 || f.width > max || f.height > max {
            return Err(format!(
                "kich thuoc anh {}x{} khong hop le (toi da {max})",
                f.width, f.height
            ));
        }
        let e = |e: windows::core::Error| e.to_string();
        let writer = DataWriter::new().map_err(e)?;
        writer.WriteBytes(&f.bgra).map_err(e)?;
        let buffer = writer.DetachBuffer().map_err(e)?;
        let bmp = SoftwareBitmap::CreateCopyFromBuffer(
            &buffer,
            BitmapPixelFormat::Bgra8,
            f.width as i32,
            f.height as i32,
        )
        .map_err(e)?;
        let res = self.engine.RecognizeAsync(&bmp).map_err(e)?.join().map_err(e)?;

        let mut lines = Vec::new();
        for line in res.Lines().map_err(e)? {
            let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, 0.0f64, 0.0f64);
            for word in line.Words().map_err(e)? {
                let r = word.BoundingRect().map_err(e)?;
                x0 = x0.min(r.X as f64);
                y0 = y0.min(r.Y as f64);
                x1 = x1.max((r.X + r.Width) as f64);
                y1 = y1.max((r.Y + r.Height) as f64);
            }
            if x0 > x1 {
                continue;
            }
            lines.push(OcrLine {
                text: line.Text().map_err(e)?.to_string(),
                x: x0 / scale,
                y: y0 / scale,
                w: (x1 - x0) / scale,
                h: (y1 - y0) / scale,
            });
        }
        Ok(OcrOutput { lines })
    }
}

#[cfg(test)]
/// Lower-case, keep letters and digits, collapse the rest. Used to compare OCR output
/// with the expected text while ignoring punctuation and spacing noise.
pub fn normalize(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
/// Fraction of expected words found in `got` (0.0 - 1.0).
pub fn word_accuracy(expected: &str, got: &str) -> f64 {
    let exp = normalize(expected);
    let got = normalize(got);
    let mut pool: Vec<&str> = got.split(' ').collect();
    let words: Vec<&str> = exp.split(' ').filter(|w| !w.is_empty()).collect();
    if words.is_empty() {
        return 1.0;
    }
    let mut hit = 0;
    for w in &words {
        if let Some(i) = pool.iter().position(|p| p == w) {
            pool.swap_remove(i);
            hit += 1;
        }
    }
    hit as f64 / words.len() as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;
    use windows::Win32::Foundation::COLORREF;
    use windows::Win32::Graphics::Gdi::*;
    use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize};
    use windows::core::w;

    /// Render `text` with GDI. `outline` draws a black border like game subtitles do.
    fn render(text: &str, px: i32, fg: u32, bg: [u8; 3], outline: bool) -> Frame {
        let (w, h) = (text.chars().count() as i32 * px * 6 / 10 + 80, px * 2 + 40);
        unsafe {
            let dc = CreateCompatibleDC(None);
            let bi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w,
                    biHeight: -h,
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
            let buf = std::slice::from_raw_parts_mut(bits as *mut u8, (w * h * 4) as usize);
            for (i, p) in buf.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                // light vertical gradient so the background is not perfectly flat
                let g = ((i as i32 / w) * 20 / h) as u8;
                p.copy_from_slice(&[bg[0].saturating_add(g), bg[1].saturating_add(g), bg[2].saturating_add(g), 255]);
            }
            let font = CreateFontW(
                px, 0, 0, 0, 700, 0, 0, 0,
                FONT_CHARSET(0),
                FONT_OUTPUT_PRECISION(0),
                FONT_CLIP_PRECISION(0),
                ANTIALIASED_QUALITY,
                0,
                w!("Arial"),
            );
            let oldf = SelectObject(dc, font.into());
            SetBkMode(dc, TRANSPARENT);
            let t: Vec<u16> = text.encode_utf16().collect();
            if outline {
                SetTextColor(dc, COLORREF(0));
                for (dx, dy) in [(-2, -2), (2, -2), (-2, 2), (2, 2), (0, -2), (0, 2), (-2, 0), (2, 0)] {
                    let _ = TextOutW(dc, 40 + dx, 20 + dy, &t);
                }
            }
            SetTextColor(dc, COLORREF(fg));
            let _ = TextOutW(dc, 40, 20, &t);
            let _ = GdiFlush();
            let mut out = buf.to_vec();
            for p in out.as_chunks_mut::<4>().0 {
                p[3] = 255;
            }
            SelectObject(dc, oldf);
            SelectObject(dc, old);
            let _ = DeleteObject(font.into());
            let _ = DeleteObject(bmp.into());
            let _ = DeleteDC(dc);
            Frame { width: w as u32, height: h as u32, bgra: out }
        }
    }

    fn engine() -> Ocr {
        let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
        Ocr::new().expect("can goi OCR tieng Anh de chay test")
    }

    const WHITE: u32 = 0x00FFFFFF;
    const YELLOW: u32 = 0x0000E0FF; // BGR order in COLORREF

    #[test]
    fn normalize_ignores_case_and_punctuation() {
        assert_eq!(normalize("  Buy a NEW,  outfit! "), "buy a new outfit");
    }

    #[test]
    fn accuracy_counts_words() {
        assert_eq!(word_accuracy("buy a new outfit", "Buy a new outfit."), 1.0);
        assert_eq!(word_accuracy("buy a new outfit", "buy a outfit"), 0.75);
        assert_eq!(word_accuracy("", "anything"), 1.0);
    }

    #[test]
    fn upscale_keeps_flat_color_and_scales_size() {
        let f = Frame { width: 3, height: 2, bgra: vec![100; 3 * 2 * 4] };
        let u = upscale(&f, 2);
        assert_eq!((u.width, u.height), (6, 4));
        assert!(u.bgra.iter().all(|&b| b == 100));
    }

    #[test]
    fn rejects_oversized_image() {
        let ocr = engine();
        let big = Ocr::max_dimension() + 1;
        let f = Frame { width: big, height: 4, bgra: vec![0; big as usize * 16] };
        assert!(ocr.recognize(&f).is_err());
    }

    #[test]
    fn reads_plain_subtitle() {
        let ocr = engine();
        let text = "Buy a new outfit";
        let f = render(text, 36, WHITE, [30, 30, 30], false);
        let out = ocr.recognize(&f).unwrap();
        assert!(word_accuracy(text, &out.text()) >= 0.99, "doc duoc: {:?}", out.text());
        assert_eq!(out.lines.len(), 1);
        assert!(out.lines[0].w > 100.0 && out.lines[0].h > 10.0);
    }

    #[test]
    fn reads_outlined_subtitle_on_light_background() {
        let ocr = engine();
        let text = "Get in the car, we are leaving Los Santos tonight.";
        let f = render(text, 34, YELLOW, [150, 160, 170], true);
        let out = ocr.recognize(&f).unwrap();
        assert!(word_accuracy(text, &out.text()) >= 0.8, "doc duoc: {:?}", out.text());
    }

    #[test]
    fn reads_across_font_sizes_and_reports_speed() {
        let ocr = engine();
        let text = "Press X to continue the mission";
        for px in [18, 20, 28, 36, 48] {
            let f = render(text, px, WHITE, [20, 40, 60], true);
            let _ = ocr.recognize(&f).unwrap(); // warm up
            let t = Instant::now();
            let out = ocr.recognize(&f).unwrap();
            let ms = t.elapsed().as_secs_f64() * 1000.0;
            let acc = word_accuracy(text, &out.text());
            println!("font {px}px ({}x{}): {ms:.1} ms, chinh xac {:.0}%, doc: {:?}", f.width, f.height, acc * 100.0, out.text());
            assert!(acc >= 0.8, "font {px}px doc sai: {:?}", out.text());
        }
    }
}
