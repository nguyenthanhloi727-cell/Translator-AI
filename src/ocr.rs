//! English OCR through the Windows.Media.Ocr engine that ships with Windows.
//! Runs on the CPU, no model download, nothing leaves the machine.

use crate::frame::Frame;
use windows::Globalization::Language;
use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Storage::Streams::DataWriter;
use windows::core::HSTRING;

/// One recognized line and its bounding box (pixels of the image given to `recognize`).
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
}

impl Ocr {
    /// English engine. Fails with a readable message when the OCR language pack is missing.
    pub fn new() -> Result<Self, String> {
        let lang = Language::CreateLanguage(&HSTRING::from("en-US")).map_err(|e| e.to_string())?;
        let engine = OcrEngine::TryCreateFromLanguage(&lang)
            .or_else(|_| OcrEngine::TryCreateFromUserProfileLanguages())
            .map_err(|_| NO_ENGINE.to_string())?;
        Ok(Self { engine })
    }

    pub fn max_dimension() -> u32 {
        OcrEngine::MaxImageDimension().unwrap_or(2600)
    }

    pub fn recognize(&self, f: &Frame) -> Result<Vec<OcrLine>, String> {
        let max = Self::max_dimension();
        if f.width > max || f.height > max {
            return Err(format!("anh {}x{} vuot gioi han OCR {max}px", f.width, f.height));
        }
        let e = |e: windows::core::Error| e.to_string();
        let writer = DataWriter::new().map_err(e)?;
        writer.WriteBytes(&f.bgra).map_err(e)?;
        let buffer = writer.DetachBuffer().map_err(e)?;
        let bitmap = SoftwareBitmap::CreateCopyFromBuffer(
            &buffer,
            BitmapPixelFormat::Bgra8,
            f.width as i32,
            f.height as i32,
        )
        .map_err(e)?;
        let result = self.engine.RecognizeAsync(&bitmap).map_err(e)?.join().map_err(e)?;

        let mut lines = Vec::new();
        for line in result.Lines().map_err(e)? {
            let text = line.Text().map_err(e)?.to_string();
            let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, 0f32, 0f32);
            for word in line.Words().map_err(e)? {
                let r = word.BoundingRect().map_err(e)?;
                x0 = x0.min(r.X);
                y0 = y0.min(r.Y);
                x1 = x1.max(r.X + r.Width);
                y1 = y1.max(r.Y + r.Height);
            }
            if text.trim().is_empty() || x1 <= x0 {
                continue;
            }
            lines.push(OcrLine { text, x: x0, y: y0, w: x1 - x0, h: y1 - y0 });
        }
        Ok(lines)
    }
}

const NO_ENGINE: &str = "may chua co goi ngon ngu OCR tieng Anh. Cai bang: Settings > Time & language > \
Language & region > English (US) > Language options > bat 'Optical character recognition'";

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;
    use windows::Win32::Graphics::Gdi::*;
    use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize};
    use windows::core::w;

    /// Render `text` with GDI (Arial) as `fg` on `bg` (0xRRGGBB) at font height `px`.
    fn render(text: &str, px: i32, fg: u32, bg: u32) -> Frame {
        let (w, h) = ((text.len() as i32 * px).max(200), px * 2 + 20);
        let bmi = BITMAPINFO {
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
        let rgb = |c: u32| windows::Win32::Foundation::COLORREF(((c & 0xff) << 16) | (c & 0xff00) | (c >> 16));
        unsafe {
            let dc = CreateCompatibleDC(None);
            let mut bits = std::ptr::null_mut();
            let bmp = CreateDIBSection(Some(dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0).unwrap();
            let old = SelectObject(dc, bmp.into());
            let font = CreateFontW(
                -px, 0, 0, 0, 700, 0, 0, 0, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS,
                CLIP_DEFAULT_PRECIS, ANTIALIASED_QUALITY, 0, w!("Arial"),
            );
            let oldf = SelectObject(dc, font.into());
            SetBkColor(dc, rgb(bg));
            SetTextColor(dc, rgb(fg));
            let wide: Vec<u16> = text.encode_utf16().collect();
            let _ = ExtTextOutW(dc, 10, 10, ETO_OPAQUE, Some(&windows::Win32::Foundation::RECT { left: 0, top: 0, right: w, bottom: h }), windows::core::PCWSTR(wide.as_ptr()), wide.len() as u32, None);
            let mut bgra = std::slice::from_raw_parts(bits as *const u8, (w * h * 4) as usize).to_vec();
            bgra.as_chunks_mut::<4>().0.iter_mut().for_each(|p| p[3] = 255);
            SelectObject(dc, oldf);
            SelectObject(dc, old);
            let _ = DeleteObject(font.into());
            let _ = DeleteObject(bmp.into());
            let _ = DeleteDC(dc);
            Frame { width: w as u32, height: h as u32, bgra }
        }
    }

    fn join_text(lines: &[OcrLine]) -> String {
        lines.iter().map(|l| l.text.trim()).collect::<Vec<_>>().join(" ")
    }

    fn engine() -> Ocr {
        let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
        Ocr::new().expect("can goi ngon ngu OCR tieng Anh")
    }

    fn norm(s: &str) -> String {
        s.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect()
    }

    #[test]
    fn reads_white_text_on_dark_at_subtitle_sizes() {
        let ocr = engine();
        let text = "Buy a new outfit";
        for px in [24, 32, 48] {
            let lines = ocr.recognize(&render(text, px, 0xffffff, 0x202020)).unwrap();
            assert_eq!(norm(&join_text(&lines)), norm(text), "co chu {px}px");
            assert!(lines[0].w > 0.0 && lines[0].h > 0.0);
        }
    }

    #[test]
    fn reads_dark_text_on_light_and_yellow_on_busy_color() {
        let ocr = engine();
        let text = "Meet me at the pier";
        let a = ocr.recognize(&render(text, 32, 0x101010, 0xf0f0f0)).unwrap();
        assert_eq!(norm(&join_text(&a)), norm(text));
        let b = ocr.recognize(&render(text, 32, 0xffe000, 0x405070)).unwrap();
        assert_eq!(norm(&join_text(&b)), norm(text));
    }

    #[test]
    fn upscale_x2_rescues_small_text() {
        let ocr = engine();
        let text = "Buy a new outfit";
        for px in [10, 12, 14, 16, 20] {
            let f = render(text, px, 0xffffff, 0x202020);
            for k in [1, 2, 3] {
                let got = ocr.recognize(&crate::frame::upscale(&f, k)).unwrap();
                let ok = norm(&join_text(&got)) == norm(text);
                println!("chu {px}px x{k}: {} -> '{}'", if ok { "DUNG" } else { "sai" }, join_text(&got));
                assert!(ok || k == 1, "chu {px}px phong x{k} van sai");
            }
        }
    }

    #[test]
    fn blank_image_gives_no_lines() {
        let ocr = engine();
        let f = Frame { width: 300, height: 60, bgra: vec![30; 300 * 60 * 4] };
        assert!(ocr.recognize(&f).unwrap().is_empty());
    }

    #[test]
    fn rejects_oversized_image() {
        let ocr = engine();
        let w = Ocr::max_dimension() + 1;
        let f = Frame { width: w, height: 1, bgra: vec![0; w as usize * 4] };
        assert!(ocr.recognize(&f).is_err());
    }

    #[test]
    fn speed_of_subtitle_strip_is_reported() {
        let ocr = engine();
        let f = render("They are going to find us if we stay here", 32, 0xffffff, 0x303030);
        ocr.recognize(&f).unwrap(); // warm up
        let t = Instant::now();
        for _ in 0..10 {
            ocr.recognize(&f).unwrap();
        }
        let ms = t.elapsed().as_secs_f64() * 100.0;
        println!("OCR {}x{}: {ms:.1} ms/lan", f.width, f.height);
    }
}
