//! English OCR through the built-in Windows.Media.Ocr engine (CPU, no model download).

use crate::frame::Frame;
use windows::Globalization::Language;
use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Storage::Streams::DataWriter;
use windows::core::HSTRING;

pub struct Line {
    pub text: String,
    /// Bounding box in the coordinates of the input frame.
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

pub struct Ocr {
    engine: OcrEngine,
}

/// Subtitle areas shorter than this (px) have small glyphs that read badly, so enlarge them.
const SMALL_AREA_H: u32 = 100;

impl Ocr {
    pub fn new() -> Result<Self, String> {
        let lang = Language::CreateLanguage(&HSTRING::from("en-US")).map_err(|e| e.to_string())?;
        if !OcrEngine::IsLanguageSupported(&lang).unwrap_or(false) {
            return Err("may thieu goi ngon ngu OCR tieng Anh. Cai bang (PowerShell quan tri): \
Add-WindowsCapability -Online -Name Language.OCR~~~en-US~0.0.1.0"
                .into());
        }
        let engine = OcrEngine::TryCreateFromLanguage(&lang).map_err(|e| e.to_string())?;
        Ok(Self { engine })
    }

    /// Recognise text. `scale` enlarges the image first (1 = as is); positions are
    /// reported back in the original frame coordinates.
    pub fn recognize(&self, frame: &Frame, scale: u32) -> Result<Vec<Line>, String> {
        let scale = scale.max(1);
        let max = OcrEngine::MaxImageDimension().unwrap_or(2600);
        if frame.width * scale > max || frame.height * scale > max {
            return Err(format!("anh qua lon cho OCR (toi da {max}px moi canh); hay dung --region"));
        }
        let scaled;
        let (w, h, data): (u32, u32, &[u8]) = if scale == 1 {
            (frame.width, frame.height, &frame.bgra)
        } else {
            scaled = upscale(frame, scale);
            (frame.width * scale, frame.height * scale, &scaled)
        };

        let writer = DataWriter::new().map_err(|e| e.to_string())?;
        writer.WriteBytes(data).map_err(|e| e.to_string())?;
        let buf = writer.DetachBuffer().map_err(|e| e.to_string())?;
        let bmp = SoftwareBitmap::CreateCopyFromBuffer(&buf, BitmapPixelFormat::Bgra8, w as i32, h as i32)
            .map_err(|e| e.to_string())?;
        let result = self
            .engine
            .RecognizeAsync(&bmp)
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
            if x1 <= x0 {
                continue;
            }
            let s = scale as f32;
            out.push(Line {
                text,
                x: (x0 / s).max(0.0) as u32,
                y: (y0 / s).max(0.0) as u32,
                w: ((x1 - x0) / s) as u32,
                h: ((y1 - y0) / s) as u32,
            });
        }
        Ok(out)
    }
}

/// Pick an enlargement factor from the height of the subtitle area.
pub fn auto_scale(frame_h: u32) -> u32 {
    if frame_h < SMALL_AREA_H { 2 } else { 1 }
}

/// Bilinear enlargement by an integer factor.
fn upscale(f: &Frame, k: u32) -> Vec<u8> {
    let (w, h) = (f.width as usize, f.height as usize);
    let (nw, nh) = (w * k as usize, h * k as usize);
    let mut out = vec![255u8; nw * nh * 4];
    let kf = k as f32;
    for y in 0..nh {
        let fy = ((y as f32 + 0.5) / kf - 0.5).clamp(0.0, (h - 1) as f32);
        let (y0, ty) = (fy as usize, fy.fract());
        let y1 = (y0 + 1).min(h - 1);
        for x in 0..nw {
            let fx = ((x as f32 + 0.5) / kf - 0.5).clamp(0.0, (w - 1) as f32);
            let (x0, tx) = (fx as usize, fx.fract());
            let x1 = (x0 + 1).min(w - 1);
            for c in 0..3 {
                let p = |xx: usize, yy: usize| f.bgra[(yy * w + xx) * 4 + c] as f32;
                let top = p(x0, y0) * (1.0 - tx) + p(x1, y0) * tx;
                let bot = p(x0, y1) * (1.0 - tx) + p(x1, y1) * tx;
                out[(y * nw + x) * 4 + c] = (top * (1.0 - ty) + bot * ty + 0.5) as u8;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;
    use windows::Win32::Foundation::{COLORREF, RECT};
    use windows::Win32::Graphics::Gdi::*;
    use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize};
    use windows::core::w;

    /// Render `text` with GDI: `fg`/`bg` are 0x00BBGGRR colours.
    fn render(text: &str, px: i32, fg: u32, bg: u32, w: i32, h: i32, shadow: bool) -> Frame {
        unsafe {
            let dc = CreateCompatibleDC(None);
            let bi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w,
                    biHeight: -h,
                    biPlanes: 1,
                    biBitCount: 32,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits = std::ptr::null_mut();
            let bmp = CreateDIBSection(Some(dc), &bi, DIB_RGB_COLORS, &mut bits, None, 0).unwrap();
            let old = SelectObject(dc, bmp.into());
            let font = CreateFontW(
                -px, 0, 0, 0, 700, 0, 0, 0, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS,
                CLIP_DEFAULT_PRECIS, ANTIALIASED_QUALITY, 0, w!("Arial"),
            );
            let oldf = SelectObject(dc, font.into());
            let brush = CreateSolidBrush(COLORREF(bg));
            FillRect(dc, &RECT { left: 0, top: 0, right: w, bottom: h }, brush);
            SetBkMode(dc, TRANSPARENT);
            let wide: Vec<u16> = text.encode_utf16().collect();
            if shadow {
                SetTextColor(dc, COLORREF(0));
                let _ = TextOutW(dc, 12, 10, &wide);
            }
            SetTextColor(dc, COLORREF(fg));
            let _ = TextOutW(dc, 10, 8, &wide);
            let mut bgra = std::slice::from_raw_parts(bits as *const u8, (w * h * 4) as usize).to_vec();
            for a in bgra.as_chunks_mut::<4>().0 {
                a[3] = 255;
            }
            SelectObject(dc, oldf);
            SelectObject(dc, old);
            let _ = DeleteObject(font.into());
            let _ = DeleteObject(brush.into());
            let _ = DeleteObject(bmp.into());
            let _ = DeleteDC(dc);
            Frame { width: w as u32, height: h as u32, bgra }
        }
    }

    fn norm(s: &str) -> String {
        s.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase()
    }

    fn engine() -> Ocr {
        let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
        Ocr::new().expect("may can goi OCR tieng Anh")
    }

    fn read(ocr: &Ocr, f: &Frame, scale: u32) -> String {
        ocr.recognize(f, scale).unwrap().iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join(" ")
    }

    #[test]
    fn reads_white_text_on_dark() {
        let ocr = engine();
        let f = render("Buy a new outfit", 36, 0xFFFFFF, 0x202020, 600, 70, false);
        let lines = ocr.recognize(&f, 1).unwrap();
        assert_eq!(norm(&lines.iter().map(|l| l.text.clone()).collect::<String>()), "buyanewoutfit");
        let l = &lines[0];
        assert!(l.x < 40 && l.y < 30 && l.w > 200 && l.h > 15, "box {} {} {} {}", l.x, l.y, l.w, l.h);
    }

    #[test]
    fn reads_dark_on_light_and_shadowed() {
        let ocr = engine();
        let a = render("Los Santos is waiting", 32, 0x101010, 0xF0F0F0, 600, 64, false);
        assert_eq!(norm(&read(&ocr, &a, 1)), "lossantosiswaiting");
        let b = render("Get in the car now", 32, 0xFFFFFF, 0x806040, 600, 64, true);
        assert_eq!(norm(&read(&ocr, &b, 1)), "getinthecarnow");
    }

    #[test]
    fn small_text_needs_upscale_and_scale_maps_boxes_back() {
        let ocr = engine();
        let f = render("Meet me at the docks", 16, 0xFFFFFF, 0x303030, 300, 36, false);
        let s = auto_scale(f.height);
        assert_eq!(s, 2);
        let lines = ocr.recognize(&f, s).unwrap();
        assert_eq!(norm(&lines.iter().map(|l| l.text.clone()).collect::<String>()), "meetmeatthedocks");
        assert!(lines[0].w < 300 && lines[0].h < 36);
    }

    #[test]
    fn blank_image_gives_no_text_and_oversize_is_rejected() {
        let ocr = engine();
        let f = Frame { width: 200, height: 40, bgra: vec![255; 200 * 40 * 4] };
        assert!(ocr.recognize(&f, 1).unwrap().is_empty());
        let m = OcrEngine::MaxImageDimension().unwrap();
        println!("MaxImageDimension = {m}");
        let big = Frame { width: m + 1, height: 2, bgra: vec![0; (m as usize + 1) * 2 * 4] };
        assert!(ocr.recognize(&big, 1).is_err());
    }

    #[test]
    fn speed_on_subtitle_area() {
        let ocr = engine();
        let f = render("I told you, never come back to this town", 34, 0xFFFFFF, 0x202020, 1200, 90, true);
        let _ = ocr.recognize(&f, 1).unwrap(); // warm up
        let n = 20;
        let t = Instant::now();
        for _ in 0..n {
            let _ = ocr.recognize(&f, 1).unwrap();
        }
        let ms = t.elapsed().as_secs_f64() * 1000.0 / n as f64;
        println!("OCR 1200x90: tb {ms:.1} ms");
        assert_eq!(norm(&read(&ocr, &f, 1)), "itoldyouneverco mebacktothistown".replace(' ', ""));
        assert!(ms < 300.0, "OCR qua cham: {ms} ms");
    }

    #[test]
    fn upscale_keeps_size_and_flat_colour() {
        let f = Frame { width: 2, height: 2, bgra: vec![100; 16] };
        let u = upscale(&f, 3);
        assert_eq!(u.len(), 6 * 6 * 4);
        assert!(u.as_chunks::<4>().0.iter().all(|p| p[0] == 100));
    }
}
