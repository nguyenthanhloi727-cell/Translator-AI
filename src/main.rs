mod capture;
mod frame;
mod ocr;

use capture::Target;
use frame::{Frame, Region, decode_bmp, encode_bmp};
use std::sync::mpsc::{RecvTimeoutError, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize};

const USAGE: &str = "translator-ai capture [--list] [--window <ten>] [--monitor] \
[--region x,y,w,h] [--seconds N] [--save file.bmp]
       translator-ai ocr (--image file.bmp | [--window <ten>] [--monitor]) [--region x,y,w,h] [--seconds N] [--scale N]";

struct Args {
    list: bool,
    window: Option<String>,
    region: Option<Region>,
    seconds: u64,
    save: Option<String>,
    image: Option<String>,
    scale: u32,
}

fn parse_args() -> Result<(String, Args), String> {
    let mut a =
        Args { list: false, window: None, region: None, seconds: 5, save: None, image: None, scale: 1 };
    let mut it = std::env::args().skip(1);
    let cmd = it.next().unwrap_or_default();
    if cmd != "capture" && cmd != "ocr" {
        return Err(USAGE.into());
    }
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
            "--scale" => a.scale = val("--scale")?.parse().map_err(|_| "--scale phai la so")?,
            _ => return Err(USAGE.into()),
        }
    }
    Ok((cmd, a))
}

fn run_ocr(args: Args) -> Result<(), String> {
    let ocr = ocr::Ocr::new()?;
    let show = |lines: &[ocr::OcrLine]| {
        for l in lines {
            println!("  [{:.0},{:.0} {:.0}x{:.0}] {}", l.x, l.y, l.w, l.h, l.text);
        }
    };

    if let Some(path) = &args.image {
        let data = std::fs::read(path).map_err(|e| e.to_string())?;
        let mut frame = decode_bmp(&data)?;
        if let Some(r) = args.region {
            let r = r.clamp(frame.width, frame.height).ok_or("region nam ngoai anh")?;
            frame = crop(&frame, r);
        }
        let _ = ocr.recognize(&frame, args.scale)?; // khoi dong engine, khong tinh gio
        let t = Instant::now();
        let lines = ocr.recognize(&frame, args.scale)?;
        println!("OCR {}x{} (scale {}): {:.1} ms, {} dong", frame.width, frame.height, args.scale, t.elapsed().as_secs_f64() * 1000.0, lines.len());
        show(&lines);
        return Ok(());
    }

    let target = match &args.window {
        Some(name) => Target::Window(
            capture::find_window(name).ok_or(format!("khong tim thay cua so chua '{name}'"))?,
        ),
        None => Target::PrimaryMonitor,
    };
    let (tx, rx) = channel();
    let last: Arc<Mutex<Option<Frame>>> = Arc::new(Mutex::new(None));
    let cap = capture::start(target, args.region, tx, last.clone()).map_err(|e| e.to_string())?;

    let end = Instant::now() + Duration::from_secs(args.seconds);
    let (mut frames, mut runs, mut sum, mut max) = (0u32, 0u32, 0.0f64, 0.0f64);
    let mut prev_text = String::new();
    while let Some(left) = end.checked_duration_since(Instant::now()) {
        let Ok(stat) = rx.recv_timeout(left) else { break };
        frames += 1;
        // Gom cac khung da den trong luc OCR chay: chi quan tam khung moi nhat.
        let mut changed = stat.changed;
        while let Ok(s) = rx.try_recv() {
            frames += 1;
            changed |= s.changed;
        }
        if !changed {
            continue; // vung khong doi: bo qua OCR
        }
        let Some(frame) = last.lock().unwrap().take() else { continue };
        let t = Instant::now();
        let lines = ocr.recognize(&frame, args.scale)?;
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        runs += 1;
        sum += ms;
        max = max.max(ms);
        let text: String = lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join(" | ");
        if text != prev_text {
            println!("[{ms:.1} ms] {text}");
            prev_text = text;
        }
    }
    cap.stop();
    println!("khung nhan duoc: {frames}, so lan OCR: {runs}");
    if runs > 0 {
        println!("OCR (ms): tb {:.1}, max {:.1}", sum / runs as f64, max);
    }
    if frames == 0 {
        return Err("khong nhan duoc khung hinh nao".into());
    }
    Ok(())
}

fn crop(f: &Frame, r: Region) -> Frame {
    let mut bgra = Vec::with_capacity((r.w * r.h * 4) as usize);
    for y in r.y..r.y + r.h {
        let start = ((y * f.width + r.x) * 4) as usize;
        bgra.extend_from_slice(&f.bgra[start..start + (r.w * 4) as usize]);
    }
    Frame { width: r.w, height: r.h, bgra }
}

fn run() -> Result<(), String> {
    let (cmd, args) = parse_args()?;
    unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.map_err(|e| e.to_string())?;

    if cmd == "ocr" {
        return run_ocr(args);
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

    let (tx, rx) = channel();
    let last: Arc<Mutex<Option<Frame>>> = Arc::new(Mutex::new(None));
    let cap = capture::start(target, args.region, tx, last.clone()).map_err(|e| e.to_string())?;

    let end = Instant::now() + Duration::from_secs(args.seconds);
    let (mut frames, mut changed) = (0u32, 0u32);
    let (mut arrive_sum, mut arrive_max, mut copy_sum, mut copy_max) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    while let Some(left) = end.checked_duration_since(Instant::now()) {
        match rx.recv_timeout(left) {
            Ok(s) => {
                frames += 1;
                changed += s.changed as u32;
                arrive_sum += s.arrive_ms;
                arrive_max = arrive_max.max(s.arrive_ms);
                copy_sum += s.copy_ms;
                copy_max = copy_max.max(s.copy_ms);
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
