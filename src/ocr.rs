//! English OCR on a `Frame` using the OCR engine built into Windows (CPU, no model download).

use crate::frame::Frame;
use windows::Globalization::Language;
use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Storage::Streams::DataWriter;
use windows::core::HSTRING;

/// One recognized line of text and its bounding box (pixels of the frame given to `recognize`).
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

pub struct Ocr {
    engine: OcrEngine,
}

const MISSING_PACK: &str = "may chua co goi ngon ngu OCR tieng Anh. Cai bang: Settings, Time & language, \
Language & region, English (United States), Language options, cai 'Optical character recognition' \
(hoac PowerShell quyen admin: Add-WindowsCapability -Online -Name Language.OCR~~~en-US~0.0.1.0)";

impl Ocr {
    /// English engine. Fails with a clear message if the Windows English OCR language pack is missing.
    pub fn new() -> Result<Self, String> {
        let lang = Language::CreateLanguage(&HSTRING::from("en-US")).map_err(|e| e.to_string())?;
        if !OcrEngine::IsLanguageSupported(&lang).unwrap_or(false) {
            return Err(MISSING_PACK.into());
        }
        let engine = OcrEngine::TryCreateFromLanguage(&lang).map_err(|e| e.to_string())?;
        Ok(Self { engine })
    }

    /// Read the text in `f`. The frame is used as is; scale small text up before calling.
    pub fn recognize(&self, f: &Frame) -> Result<Vec<Line>, String> {
        let max = OcrEngine::MaxImageDimension().unwrap_or(10_000);
        if f.width > max || f.height > max {
            return Err(format!("anh qua lon cho OCR (toi da {max} px moi canh)"));
        }
        let bitmap = to_bitmap(f).map_err(|e| e.to_string())?;
        let result = self
            .engine
            .RecognizeAsync(&bitmap)
            .and_then(|op| op.join())
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for line in result.Lines().map_err(|e| e.to_string())? {
            let text = line.Text().map_err(|e| e.to_string())?.to_string();
            let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, 0f32, 0f32);
            for word in line.Words().map_err(|e| e.to_string())? {
                let r = word.BoundingRect().map_err(|e| e.to_string())?;
                x0 = x0.min(r.X);
                y0 = y0.min(r.Y);
                x1 = x1.max(r.X + r.Width);
                y1 = y1.max(r.Y + r.Height);
            }
            if x0 <= x1 {
                out.push(Line { text, x: x0, y: y0, w: x1 - x0, h: y1 - y0 });
            }
        }
        Ok(out)
    }
}

fn to_bitmap(f: &Frame) -> windows::core::Result<SoftwareBitmap> {
    let writer = DataWriter::new()?;
    writer.WriteBytes(&f.bgra)?;
    let buffer = writer.DetachBuffer()?;
    SoftwareBitmap::CreateCopyFromBuffer(
        &buffer,
        BitmapPixelFormat::Bgra8,
        f.width as i32,
        f.height as i32,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::scale_up;
    use std::time::Instant;
    use windows::Win32::Foundation::{COLORREF, RECT};
    use windows::Win32::Graphics::Gdi::*;
    use windows::core::w;

    /// Render `text` with GDI: `px` tall font, colour `fg` on `bg` (0xRRGGBB).
    fn render(text: &str, px: i32, fg: u32, bg: u32, width: u32, height: u32) -> Frame {
        unsafe {
            let dc = CreateCompatibleDC(None);
            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width as i32,
                    biHeight: -(height as i32),
                    biPlanes: 1,
                    biBitCount: 32,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits = std::ptr::null_mut();
            let dib = CreateDIBSection(Some(dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0).unwrap();
            let old = SelectObject(dc, dib.into());
            let font = CreateFontW(
                -px,
                0,
                0,
                0,
                700,
                0,
                0,
                0,
                FONT_CHARSET(0),
                FONT_OUTPUT_PRECISION(0),
                FONT_CLIP_PRECISION(0),
                FONT_QUALITY(ANTIALIASED_QUALITY.0),
                0,
                w!("Arial"),
            );
            let oldf = SelectObject(dc, font.into());
            let colorref = |c: u32| COLORREF(((c & 0xff) << 16) | (c & 0xff00) | (c >> 16));
            let brush = CreateSolidBrush(colorref(bg));
            let rc = RECT { left: 0, top: 0, right: width as i32, bottom: height as i32 };
            FillRect(dc, &rc, brush);
            SetBkMode(dc, TRANSPARENT);
            SetTextColor(dc, colorref(fg));
            let wide: Vec<u16> = text.encode_utf16().collect();
            let _ = TextOutW(dc, 10, 6, &wide);
            let n = (width * height * 4) as usize;
            let mut bgra = std::slice::from_raw_parts(bits as *const u8, n).to_vec();
            bgra.as_chunks_mut::<4>().0.iter_mut().for_each(|p| p[3] = 255);
            SelectObject(dc, oldf);
            SelectObject(dc, old);
            let _ = DeleteObject(font.into());
            let _ = DeleteObject(brush.into());
            let _ = DeleteObject(dib.into());
            let _ = DeleteDC(dc);
            Frame { width, height, bgra }
        }
    }

    fn norm(lines: &[Line]) -> String {
        lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join(" ").to_lowercase()
    }

    fn engine() -> Ocr {
        Ocr::new().expect("can goi ngon ngu OCR tieng Anh")
    }

    #[test]
    fn reads_subtitle_text_white_on_dark() {
        let f = render("Buy a new outfit at Ponsonbys", 36, 0xffffff, 0x202020, 700, 60);
        let lines = engine().recognize(&f).unwrap();
        let t = norm(&lines);
        assert!(t.contains("buy a new outfit"), "doc duoc: {t:?}");
        assert!(lines[0].w > 100.0 && lines[0].h > 10.0, "bbox: {:?}", lines[0]);
    }

    #[test]
    fn reads_dark_on_light_and_yellow() {
        let ocr = engine();
        for (fg, bg) in [(0x101010, 0xf0f0f0), (0xffd800, 0x303050)] {
            let f = render("Let us go to Los Santos", 36, fg, bg, 600, 60);
            let t = norm(&ocr.recognize(&f).unwrap());
            assert!(t.contains("los santos"), "{fg:x}/{bg:x} doc duoc: {t:?}");
        }
    }

    #[test]
    fn small_text_is_read_after_scaling_up() {
        let f = render("Get in the car, we are leaving now", 14, 0xffffff, 0x303030, 400, 30);
        let big = scale_up(&f, 3);
        let t = norm(&engine().recognize(&big).unwrap());
        assert!(t.contains("get in the car"), "doc duoc sau khi phong to: {t:?}");
    }

    #[test]
    fn empty_image_gives_no_lines() {
        let f = Frame { width: 200, height: 40, bgra: vec![30; 200 * 40 * 4] };
        assert!(engine().recognize(&f).unwrap().is_empty());
    }

    /// Prints timing; run with `cargo test --release -- --nocapture ocr_speed`.
    #[test]
    fn ocr_speed() {
        let ocr = engine();
        let f = render(
            "Buy a new outfit at Ponsonbys before the heist",
            36,
            0xffffff,
            0x202020,
            1200,
            150,
        );
        if let Ok(path) = std::env::var("OCR_SAVE_SAMPLE") {
            std::fs::write(path, crate::frame::encode_bmp(&f)).unwrap();
        }
        ocr.recognize(&f).unwrap();
        let n = 20;
        let t0 = Instant::now();
        for _ in 0..n {
            ocr.recognize(&f).unwrap();
        }
        let ms = t0.elapsed().as_secs_f64() * 1000.0 / n as f64;
        println!("OCR vung 1200x150: tb {ms:.1} ms");
        assert!(ms < 500.0);
    }
}
