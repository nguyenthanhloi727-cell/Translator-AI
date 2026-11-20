mod capture;
mod frame;
mod translate;

use capture::Target;
use frame::{Frame, Region, encode_bmp};
use std::sync::mpsc::{RecvTimeoutError, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize};

const USAGE: &str = "translator-ai capture [--list] [--window <ten>] [--monitor] \
[--region x,y,w,h] [--seconds N] [--save file.bmp]";

struct Args {
    list: bool,
    window: Option<String>,
    region: Option<Region>,
    seconds: u64,
    save: Option<String>,
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args { list: false, window: None, region: None, seconds: 5, save: None };
    let mut it = std::env::args().skip(1);
    if it.next().as_deref() != Some("capture") {
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
            _ => return Err(USAGE.into()),
        }
    }
    Ok(a)
}

fn run() -> Result<(), String> {
    match std::env::args().nth(1).as_deref() {
        Some("translate") => return run_translate(),
        Some("model") => return run_model(),
        _ => {}
    }
    let args = parse_args()?;
    unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.map_err(|e| e.to_string())?;

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

fn model_dir(args: &[String]) -> std::path::PathBuf {
    let pos = args.iter().position(|a| a == "--model");
    pos.and_then(|i| args.get(i + 1))
        .map_or(translate::DEFAULT_MODEL_DIR.into(), Into::into)
}

fn run_model() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(2).collect();
    if args.first().map(String::as_str) != Some("verify") {
        return Err(USAGE.into());
    }
    let dir = model_dir(&args);
    let t = Instant::now();
    translate::verify_model(&dir)?;
    println!("model OK, SHA256 khop ({:.1} s): {}", t.elapsed().as_secs_f64(), dir.display());
    Ok(())
}

fn run_translate() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(2).collect();
    let dir = model_dir(&args);
    let mut threads = 0usize;
    let mut bench = false;
    let mut texts = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--model" => i += 1,
            "--threads" => {
                i += 1;
                threads = args.get(i).and_then(|v| v.parse().ok()).ok_or("--threads phai la so")?;
            }
            "--bench" => bench = true,
            t => texts.push(t.to_string()),
        }
        i += 1;
    }
    let t0 = Instant::now();
    let mut tr = translate::Translator::load(&dir, threads)?;
    println!("nap model (gom kiem SHA256): {:.2} s, tu dien {} muc", t0.elapsed().as_secs_f64(), tr.glossary_len());
    if texts.is_empty() {
        texts = translate::BENCH_SENTENCES.iter().map(|s| s.to_string()).collect();
    }
    let rounds = if bench { 3 } else { 1 };
    let (mut miss_ms, mut miss_n, mut hit_ms, mut hit_n) = (0.0f64, 0u32, 0.0f64, 0u32);
    for round in 0..rounds {
        for text in &texts {
            let t = Instant::now();
            let out = tr.translate(text)?;
            let ms = t.elapsed().as_secs_f64() * 1000.0;
            if out.cached {
                hit_ms += ms;
                hit_n += 1;
            } else {
                miss_ms += ms;
                miss_n += 1;
            }
            if round == 0 {
                println!("[{ms:7.1} ms{}] {text}
            => {}", if out.cached { " cache" } else { "" }, out.text);
            }
        }
    }
    if bench {
        if miss_n > 0 {
            println!("cache miss: {miss_n} cau, tb {:.1} ms", miss_ms / miss_n as f64);
        }
        if hit_n > 0 {
            println!("cache hit : {hit_n} cau, tb {:.4} ms", hit_ms / hit_n as f64);
        }
    }
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("loi: {e}");
        std::process::exit(1);
    }
}
