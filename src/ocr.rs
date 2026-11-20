//! English OCR through the Windows.Media.Ocr engine that ships with Windows (CPU, offline).

use crate::frame::{Frame, upscale};
use windows::Globalization::Language;
use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Storage::Streams::DataWriter;
use windows::core::HSTRING;

/// One recognized line of text with its box in the coordinates of the input frame.
#[derive(Clone, Debug, PartialEq)]
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
    /// English engine. Fails with a clear message if the English OCR language pack is missing.
    pub fn new() -> Result<Self, String> {
        let missing = "may chua co goi ngon ngu OCR tieng Anh. Cai bang: Settings > Time & language > \
Language & region > English (United States) > Language options > cai goi OCR \
(hoac PowerShell admin: Add-WindowsCapability -Online -Name Language.OCR~~~en-US~0.0.1.0)";
        let lang = Language::CreateLanguage(&HSTRING::from("en-US")).map_err(|e| e.to_string())?;
        if !OcrEngine::IsLanguageSupported(&lang).unwrap_or(false) {
            return Err(missing.into());
        }
        let engine = OcrEngine::TryCreateFromLanguage(&lang).map_err(|_| missing.to_string())?;
        Ok(Self { engine })
    }

    /// Recognize text. `scale` (>= 1) enlarges the image first, which helps small text;
    /// returned boxes are mapped back to the original frame size.
    pub fn recognize(&self, frame: &Frame, scale: u32) -> Result<Vec<OcrLine>, String> {
        let scale = scale.max(1);
        let max = OcrEngine::MaxImageDimension().unwrap_or(10_000);
        if frame.width * scale > max || frame.height * scale > max {
            return Err(format!("anh qua lon cho OCR (toi da {max} px moi canh sau khi phong to)"));
        }
        let scaled;
        let src = if scale > 1 {
            scaled = upscale(frame, scale);
            &scaled
        } else {
            frame
        };

        let bitmap = to_bitmap(src).map_err(|e| e.to_string())?;
        let result = self
            .engine
            .RecognizeAsync(&bitmap)
            .and_then(|op| op.join())
            .map_err(|e| e.to_string())?;

        let k = scale as f32;
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
            if x0 <= x1 && y0 <= y1 {
                out.push(OcrLine { text, x: x0 / k, y: y0 / k, w: (x1 - x0) / k, h: (y1 - y0) / k });
            }
        }
        Ok(out)
    }
}

fn to_bitmap(f: &Frame) -> windows::core::Result<SoftwareBitmap> {
    let writer = DataWriter::new()?;
    writer.WriteBytes(&f.bgra)?;
    let buffer = writer.DetachBuffer()?;
    SoftwareBitmap::CreateCopyFromBuffer(&buffer, BitmapPixelFormat::Bgra8, f.width as i32, f.height as i32)
}

/// Lowercase letters and digits only, for comparing OCR output to expected text.
#[cfg(test)]
fn normalize(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;
    use windows::Win32::Graphics::Gdi::*;
    use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize};
    use windows::core::w;

    /// Render `text` with GDI (anti-aliased, like game subtitles). `bg_noise` adds a gradient
    /// so the background is not flat.
    fn render(text: &str, face: windows::core::PCWSTR, px: i32, fg: u32, bg: u32, gradient: bool) -> Frame {
        let pad = px;
        let width = (text.len() as i32 * px * 6 / 10 + pad * 2) as u32;
        let height = (px * 2 + pad) as u32;
        unsafe {
            let hdc = CreateCompatibleDC(None);
            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width as i32,
                    biHeight: -(height as i32),
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits = std::ptr::null_mut();
            let dib = CreateDIBSection(Some(hdc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0).unwrap();
            let old = SelectObject(hdc, dib.into());
            let brush = CreateSolidBrush(windows::Win32::Foundation::COLORREF(bg));
            let rect = windows::Win32::Foundation::RECT { left: 0, top: 0, right: width as i32, bottom: height as i32 };
            FillRect(hdc, &rect, brush);
            let font = CreateFontW(
                -px, 0, 0, 0, 400, 0, 0, 0, DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS,
                CLEARTYPE_QUALITY, 0, face,
            );
            let oldf = SelectObject(hdc, font.into());
            SetBkMode(hdc, TRANSPARENT);
            SetTextColor(hdc, windows::Win32::Foundation::COLORREF(fg));
            let wide: Vec<u16> = text.encode_utf16().collect();
            let _ = TextOutW(hdc, pad, pad / 2, &wide);
            let _ = GdiFlush();
            let mut bgra = std::slice::from_raw_parts(bits as *const u8, (width * height * 4) as usize).to_vec();
            SelectObject(hdc, oldf);
            SelectObject(hdc, old);
            let _ = DeleteObject(font.into());
            let _ = DeleteObject(brush.into());
            let _ = DeleteObject(dib.into());
            let _ = DeleteDC(hdc);
            for px4 in bgra.as_chunks_mut::<4>().0 {
                px4[3] = 255;
            }
            if gradient {
                // Only touch pixels close to the background color so the text stays intact.
                let bg_b = ((bg >> 16) & 0xff) as i32;
                for y in 0..height as usize {
                    for x in 0..width as usize {
                        let p = &mut bgra[(y * width as usize + x) * 4..][..4];
                        if (p[0] as i32 - bg_b).abs() < 8 {
                            let add = (x * 60 / width as usize) as u8;
                            p[0] = p[0].saturating_add(add);
                            p[1] = p[1].saturating_add(add / 2);
                        }
                    }
                }
            }
            Frame { width, height, bgra }
        }
    }

    fn rgb(r: u32, g: u32, b: u32) -> u32 {
        r | (g << 8) | (b << 16)
    }

    fn engine() -> Ocr {
        let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
        Ocr::new().expect("OCR engine")
    }

    fn joined(lines: &[OcrLine]) -> String {
        lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join(" ")
    }

    const SENTENCES: [&str; 3] = [
        "Buy a new outfit",
        "Get in the car, we need to leave Los Santos now",
        "Michael, are you out of your mind?",
    ];

    #[test]
    fn normalize_ignores_case_and_punctuation() {
        assert_eq!(normalize("Hello, World!  1"), "helloworld1");
    }

    #[test]
    fn reads_clear_subtitles_white_on_dark() {
        let ocr = engine();
        for s in SENTENCES {
            let f = render(s, w!("Arial"), 32, rgb(255, 255, 255), rgb(20, 20, 20), false);
            let lines = ocr.recognize(&f, 1).unwrap();
            assert_eq!(normalize(&joined(&lines)), normalize(s), "da doc: {lines:?}");
        }
    }

    #[test]
    fn reads_dark_text_on_light_and_gradient() {
        let ocr = engine();
        let s = SENTENCES[1];
        let f = render(s, w!("Segoe UI"), 36, rgb(10, 10, 10), rgb(235, 235, 235), false);
        assert_eq!(normalize(&joined(&ocr.recognize(&f, 1).unwrap())), normalize(s));
        let f = render(s, w!("Arial"), 36, rgb(255, 255, 255), rgb(30, 30, 60), true);
        assert_eq!(normalize(&joined(&ocr.recognize(&f, 1).unwrap())), normalize(s));
    }

    #[test]
    fn small_text_is_helped_by_upscale() {
        let ocr = engine();
        let s = SENTENCES[1];
        let f = render(s, w!("Arial"), 12, rgb(255, 255, 255), rgb(20, 20, 20), false);
        let want = normalize(s);
        let plain = normalize(&joined(&ocr.recognize(&f, 1).unwrap()));
        let big = normalize(&joined(&ocr.recognize(&f, 3).unwrap()));
        println!("chu 12px: scale1='{plain}' scale3='{big}'");
        assert_eq!(big, want);
        assert_ne!(plain, want, "neu pass thi phong to khong con can o co chu nay");
    }

    #[test]
    fn boxes_map_back_to_frame_coordinates() {
        let ocr = engine();
        let f = render(SENTENCES[0], w!("Arial"), 32, rgb(255, 255, 255), rgb(20, 20, 20), false);
        for scale in [1, 2] {
            let l = &ocr.recognize(&f, scale).unwrap()[0];
            assert!(l.x >= 0.0 && l.y >= 0.0, "{l:?}");
            assert!(l.x + l.w <= f.width as f32 + 1.0 && l.y + l.h <= f.height as f32 + 1.0, "{l:?}");
            assert!(l.w > f.width as f32 * 0.4, "{l:?}");
        }
    }

    #[test]
    fn empty_picture_gives_no_text() {
        let ocr = engine();
        let f = Frame { width: 400, height: 80, bgra: vec![30; 400 * 80 * 4] };
        assert!(ocr.recognize(&f, 1).unwrap().is_empty());
    }

    /// Speed measurement, run with: cargo test --release bench_ocr -- --nocapture --ignored
    #[test]
    #[ignore]
    fn bench_ocr() {
        let ocr = engine();
        for (label, w_px, h_px, px) in [("1200x150, chu 32px", 1200, 150, 32), ("1600x100, chu 28px", 1600, 100, 28)] {
            let mut f = render(SENTENCES[1], w!("Arial"), px, rgb(255, 255, 255), rgb(20, 20, 20), true);
            // pad/crop to the benchmark region size
            let mut padded = vec![20u8; (w_px * h_px * 4) as usize];
            for y in 0..f.height.min(h_px) {
                for x in 0..f.width.min(w_px) {
                    let s = ((y * f.width + x) * 4) as usize;
                    let d = ((y * w_px + x) * 4) as usize;
                    padded[d..d + 4].copy_from_slice(&f.bgra[s..s + 4]);
                }
            }
            f = Frame { width: w_px, height: h_px, bgra: padded };
            let _ = ocr.recognize(&f, 1).unwrap(); // warm up
            for scale in [1, 2] {
                let mut times: Vec<f64> = (0..30)
                    .map(|_| {
                        let t = Instant::now();
                        let _ = ocr.recognize(&f, scale).unwrap();
                        t.elapsed().as_secs_f64() * 1000.0
                    })
                    .collect();
                times.sort_by(|a, b| a.partial_cmp(b).unwrap());
                let avg = times.iter().sum::<f64>() / times.len() as f64;
                println!("{label}, scale {scale}: tb {avg:.1} ms, min {:.1}, max {:.1}", times[0], times[29]);
            }
        }
    }
}
