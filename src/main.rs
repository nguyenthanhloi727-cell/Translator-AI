mod capture;
mod frame;
mod ocr;

use capture::Target;
use frame::{Frame, Region, decode_bmp, encode_bmp, scale_up};
use std::sync::mpsc::{RecvTimeoutError, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize};

const USAGE: &str = "translator-ai capture [--list] [--window <ten>] [--monitor] \
[--region x,y,w,h] [--seconds N] [--save file.bmp]
translator-ai ocr (--image file.bmp | [--window <ten>] [--monitor]) [--region x,y,w,h] \
[--seconds N] [--scale K]";

#[derive(PartialEq)]
enum Cmd {
    Capture,
    Ocr,
}

struct Args {
    cmd: Cmd,
    list: bool,
    window: Option<String>,
    region: Option<Region>,
    seconds: u64,
    save: Option<String>,
    image: Option<String>,
    scale: u32,
}

fn parse_args() -> Result<Args, String> {
    let mut it = std::env::args().skip(1);
    let cmd = match it.next().as_deref() {
        Some("capture") => Cmd::Capture,
        Some("ocr") => Cmd::Ocr,
        _ => return Err(USAGE.into()),
    };
    let mut a = Args {
        cmd,
        list: false,
        window: None,
        region: None,
        seconds: 5,
        save: None,
        image: None,
        scale: 1,
    };
    while let Some(arg) = it.next() {
        let mut val = |name: &str| it.next().ok_or(format!("{name} can gia tri"));
        match arg.as_str() {
            "--list" => a.list = true,
            "--monitor" => a.window = None,
            "--window" => a.window = Some(val("--window")?),
            "--region" => a.region = Some(Region::parse(&val("--region")?)?),
            "--seconds" => {
                a.seconds = val("--seconds")?.parse().map_err(|_| "--seconds phai la so")?
            }
            "--save" => a.save = Some(val("--save")?),
            "--image" => a.image = Some(val("--image")?),
            "--scale" => {
                a.scale = val("--scale")?.parse().map_err(|_| "--scale phai la so")?;
                if !(1..=8).contains(&a.scale) {
                    return Err("--scale tu 1 den 8".into());
                }
            }
            _ => return Err(USAGE.into()),
        }
    }
    Ok(a)
}

/// Crop a frame to `r` (already clamped to the frame).
fn crop(f: &Frame, r: Region) -> Frame {
    let mut bgra = Vec::with_capacity((r.w * r.h * 4) as usize);
    for y in r.y..r.y + r.h {
        let start = ((y * f.width + r.x) * 4) as usize;
        bgra.extend_from_slice(&f.bgra[start..start + (r.w * 4) as usize]);
    }
    Frame { width: r.w, height: r.h, bgra }
}

/// OCR one frame, print the lines and the time it took. Returns elapsed ms.
fn ocr_and_print(engine: &ocr::Ocr, f: &Frame, scale: u32) -> Result<f64, String> {
    let t0 = Instant::now();
    let big = scale_up(f, scale);
    let lines = engine.recognize(&big)?;
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    let k = scale as f32;
    for l in &lines {
        println!(
            "  [{:.0},{:.0} {:.0}x{:.0}] {}",
            l.x / k,
            l.y / k,
            l.w / k,
            l.h / k,
            l.text
        );
    }
    if lines.is_empty() {
        println!("  (khong doc duoc chu)");
    }
    println!("  OCR: {ms:.1} ms (gom phong to x{scale})");
    Ok(ms)
}

fn run_ocr_image(args: &Args, path: &str) -> Result<(), String> {
    let data = std::fs::read(path).map_err(|e| format!("khong doc duoc {path}: {e}"))?;
    let mut f = decode_bmp(&data)?;
    if let Some(r) = args.region {
        let r = r.clamp(f.width, f.height).ok_or("region nam ngoai anh")?;
        f = crop(&f, r);
    }
    let engine = ocr::Ocr::new()?;
    ocr_and_print(&engine, &f, args.scale)?; // warm up, first call is slower
    ocr_and_print(&engine, &f, args.scale)?;
    Ok(())
}

fn run() -> Result<(), String> {
    let args = parse_args()?;
    unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.map_err(|e| e.to_string())?;

    if args.cmd == Cmd::Ocr
        && let Some(path) = &args.image
    {
        return run_ocr_image(&args, path);
    }

    if args.list {
        for (_, title) in capture::list_windows() {
            println!("{title}");
        }
        return Ok(());
    }

    let target = match &args.window {
        Some(name) => Target::Window(
            capture::find_window(name).ok_or(format!("khong tim thay cua so chua '{name}'"))?,
        ),
        None => Target::PrimaryMonitor,
    };
    let engine = if args.cmd == Cmd::Ocr { Some(ocr::Ocr::new()?) } else { None };

    let (tx, rx) = channel();
    let last: Arc<Mutex<Option<Frame>>> = Arc::new(Mutex::new(None));
    let cap = capture::start(target, args.region, tx, last.clone()).map_err(|e| e.to_string())?;

    let end = Instant::now() + Duration::from_secs(args.seconds);
    let (mut frames, mut changed) = (0u32, 0u32);
    let (mut arrive_sum, mut arrive_max, mut copy_sum, mut copy_max) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    let (mut ocr_runs, mut ocr_sum, mut ocr_max) = (0u32, 0.0f64, 0.0f64);
    while let Some(left) = end.checked_duration_since(Instant::now()) {
        match rx.recv_timeout(left) {
            Ok(s) => {
                frames += 1;
                changed += s.changed as u32;
                arrive_sum += s.arrive_ms;
                arrive_max = arrive_max.max(s.arrive_ms);
                copy_sum += s.copy_ms;
                copy_max = copy_max.max(s.copy_ms);
                // Only OCR when the area changed.
                if let (Some(engine), true) = (&engine, s.changed) {
                    let frame = last.lock().unwrap().take();
                    if let Some(f) = frame {
                        println!("khung doi:");
                        let ms = ocr_and_print(engine, &f, args.scale)?;
                        ocr_runs += 1;
                        ocr_sum += ms;
                        ocr_max = ocr_max.max(ms);
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => break,
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    cap.stop();

    println!("khung hinh nhan duoc : {frames} ({:.1}/giay)", frames as f64 / args.seconds as f64);
    println!("khung co thay doi    : {changed}");
    if frames > 0 {
        let n = frames as f64;
        println!("do tre toi app (ms)  : tb {:.2}, max {:.2}", arrive_sum / n, arrive_max);
        println!("copy GPU->CPU (ms)   : tb {:.2}, max {:.2}", copy_sum / n, copy_max);
    }
    if ocr_runs > 0 {
        println!(
            "OCR (ms)             : {ocr_runs} lan, tb {:.1}, max {:.1}",
            ocr_sum / ocr_runs as f64,
            ocr_max
        );
    }
    if let Some(path) = args.save {
        let guard = last.lock().unwrap();
        let f = guard.as_ref().ok_or("chua co khung hinh de luu")?;
        std::fs::write(&path, encode_bmp(f)).map_err(|e| e.to_string())?;
        println!("da luu khung cuoi    : {path}");
    }
    if frames == 0 {
        return Err("khong nhan duoc khung hinh nao".into());
    }
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("loi: {e}");
        std::process::exit(1);
    }
}
