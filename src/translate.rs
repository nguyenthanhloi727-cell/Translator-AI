//! Dich Anh -> Viet bang NLLB-200-distilled-600M (CTranslate2, int8, CPU).
//! Thu tu: chuan hoa -> khop ca cau voi tu dien -> cache -> bao ve ten rieng -> model.

use ct2rs::tokenizers::auto::Tokenizer;
use ct2rs::{Config, TranslationOptions, Translator as Ct2};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, VecDeque};
use std::io::Read;
use std::path::Path;

const SRC_LANG: &str = "eng_Latn";
const TGT_LANG: &str = "vie_Latn";
const CACHE_CAP: usize = 512;
const MANIFEST: &str = include_str!("../model-manifest.txt");
const DEFAULT_GLOSSARY: &str = include_str!("../glossary.txt");

/// Thu muc model mac dinh (nam ngoai git, xem .gitignore).
pub const DEFAULT_MODEL_DIR: &str = "models/nllb-600m-int8";

/// Tu dien ten rieng: `en = vi` (ve phai la chu hien ra, thuong giu nguyen ten).
pub struct Glossary {
    entries: Vec<(String, String)>,
}

impl Glossary {
    pub fn parse(text: &str) -> Glossary {
        let mut entries: Vec<(String, String)> = text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .filter_map(|l| {
                let (en, vi) = l.split_once('=')?;
                Some((en.trim().to_string(), vi.trim().to_string()))
            })
            .filter(|(en, vi)| !en.is_empty() && !vi.is_empty())
            .collect();
        // Cum dai truoc de "Vice City Beach" thang "Vice City".
        entries.sort_by_key(|(en, _)| std::cmp::Reverse(en.len()));
        Glossary { entries }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Ca cau (bo dau cau cuoi) la mot muc tu dien thi tra thang.
    fn exact(&self, text: &str) -> Option<&str> {
        let t = text.trim_end_matches(['.', '!', '?', ':']);
        self.entries
            .iter()
            .find(|(en, _)| en.eq_ignore_ascii_case(t))
            .map(|(_, vi)| vi.as_str())
    }

    /// Thay ten rieng bang ma giu cho (NLLB thuong chep nguyen ten la), tra ve danh sach de khoi phuc.
    fn protect(&self, text: &str) -> (String, Vec<(String, String)>) {
        let mut out = text.to_string();
        let mut restore = Vec::new();
        for (en, vi) in &self.entries {
            while let Some(pos) = find_word(&out, en) {
                let ph = placeholder(restore.len());
                out.replace_range(pos..pos + en.len(), &ph);
                restore.push((ph, vi.clone()));
            }
        }
        (out, restore)
    }
}

/// Ten gia la dung 1 chu, de model giu nguyen.
fn placeholder(i: usize) -> String {
    const NAMES: [&str; 8] =
        ["Zorvan", "Belkis", "Tamrok", "Quenla", "Dravik", "Meluna", "Sorbek", "Hadrin"];
    NAMES[i % NAMES.len()].to_string() + if i >= NAMES.len() { "x" } else { "" }
}

/// Tim `needle` khong phan biet hoa thuong, dung nguyen tu (ASCII).
fn find_word(hay: &str, needle: &str) -> Option<usize> {
    let (h, n) = (hay.to_ascii_lowercase(), needle.to_ascii_lowercase());
    let is_word = |b: u8| b.is_ascii_alphanumeric();
    let mut from = 0;
    while let Some(p) = h[from..].find(&n) {
        let s = from + p;
        let e = s + n.len();
        let before_ok = s == 0 || !is_word(h.as_bytes()[s - 1]);
        let after_ok = e >= h.len() || !is_word(h.as_bytes()[e]);
        if before_ok && after_ok {
            return Some(s);
        }
        from = s + 1;
    }
    None
}

/// Gop dong, bo khoang trang thua (OCR hay tra nhieu dong).
pub fn normalize(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Cau mau kieu phu de GTA de benchmark nhanh.
pub const BENCH_SENTENCES: [&str; 10] = [
    "Buy a new outfit",
    "Press X to continue",
    "Lucia, we need to get out of Vice City before the cops show up.",
    "I'm not going back to prison, Jason.",
    "Let's go to Leonida Keys and lay low for a while.",
    "Hey, did you see that? Somebody just robbed the bank on Ocean Drive!",
    "Get in the car, we're running out of time.",
    "You owe me fifty grand and I want it by tomorrow.",
    "Mission failed: the target escaped.",
    "Follow the marker to the safehouse.",
];

pub struct Outcome {
    pub text: String,
    pub cached: bool,
}

/// Cache cau da dich, day thi bo cau cu nhat.
struct Cache {
    map: HashMap<String, String>,
    order: VecDeque<String>,
    cap: usize,
}

impl Cache {
    fn new(cap: usize) -> Cache {
        Cache { map: HashMap::new(), order: VecDeque::new(), cap }
    }

    fn get(&self, key: &str) -> Option<&String> {
        self.map.get(key)
    }

    fn put(&mut self, key: String, value: String) {
        if self.map.insert(key.clone(), value).is_some() {
            return;
        }
        if self.order.len() >= self.cap
            && let Some(old) = self.order.pop_front()
        {
            self.map.remove(&old);
        }
        self.order.push_back(key);
    }
}

pub struct Translator {
    ct2: Ct2<Tokenizer>,
    glossary: Glossary,
    cache: Cache,
}

impl Translator {
    /// Kiem tra SHA256 cua file model roi nap. `threads` = 0 de tu chon (toi da 6).
    pub fn load(dir: &Path, threads: usize) -> Result<Translator, String> {
        verify_model(dir)?;
        let threads = if threads > 0 {
            threads
        } else {
            // Do tren i5-14600KF: 6 luong nhanh nhat, them nua khong nhanh hon.
            std::thread::available_parallelism().map_or(4, |n| n.get().min(6))
        };
        let cfg = Config { num_threads_per_replica: threads, ..Config::default() };
        let ct2 = Ct2::new(dir, &cfg).map_err(|e| format!("khong nap duoc model: {e}"))?;
        let mut glossary = Glossary::parse(DEFAULT_GLOSSARY);
        if let Ok(extra) = std::fs::read_to_string(dir.join("glossary.txt")) {
            let user = Glossary::parse(&extra);
            glossary.entries.splice(0..0, user.entries);
            glossary.entries.sort_by_key(|(en, _)| std::cmp::Reverse(en.len()));
        }
        Ok(Translator { ct2, glossary, cache: Cache::new(CACHE_CAP) })
    }

    pub fn glossary_len(&self) -> usize {
        self.glossary.len()
    }

    pub fn translate(&mut self, text: &str) -> Result<Outcome, String> {
        let text = normalize(text);
        if text.is_empty() {
            return Ok(Outcome { text, cached: true });
        }
        if let Some(vi) = self.glossary.exact(&text) {
            return Ok(Outcome { text: vi.to_string(), cached: true });
        }
        if let Some(hit) = self.cache.get(&text) {
            return Ok(Outcome { text: hit.clone(), cached: true });
        }
        let (masked, restore) = self.glossary.protect(&text);
        let mut vi = self.run(&masked)?;
        if restore.iter().all(|(ph, _)| vi.contains(ph.as_str())) {
            for (ph, name) in &restore {
                vi = vi.replace(ph.as_str(), name);
            }
        } else if !restore.is_empty() {
            // Model lam hong ma giu cho: dich lai nguyen van, giu ten nhu model dich.
            vi = self.run(&text)?;
        }
        self.cache.put(text, vi.clone());
        Ok(Outcome { text: vi, cached: false })
    }

    fn run(&self, src: &str) -> Result<String, String> {
        let opts = TranslationOptions::<String, String> {
            beam_size: 1,
            max_decoding_length: 160,
            disable_unk: true,
            ..Default::default()
        };
        let res = self
            .ct2
            .translate_batch_with_source_and_target_prefix(
                &[src],
                &[vec![SRC_LANG]],
                &[vec![TGT_LANG]],
                &opts,
                None,
            )
            .map_err(|e| format!("loi dich: {e}"))?;
        let out = res.into_iter().next().map(|(t, _)| t).unwrap_or_default();
        Ok(out.trim().to_string())
    }
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut f = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Doi chieu SHA256 tung file model voi `model-manifest.txt` (dong: `sha256  ten_file`).
pub fn verify_model(dir: &Path) -> Result<(), String> {
    let mut checked = 0;
    for line in MANIFEST.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
        let (want, name) = line.split_once(char::is_whitespace).ok_or("manifest sai dinh dang")?;
        let got = sha256_file(&dir.join(name.trim()))?;
        if !got.eq_ignore_ascii_case(want) {
            return Err(format!("SHA256 {} khong khop (co {got}, can {want}), tai lai model", name.trim()));
        }
        checked += 1;
    }
    if checked == 0 {
        return Err("manifest rong".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_gop_dong() {
        assert_eq!(normalize("  Buy a\n new   outfit \r\n"), "Buy a new outfit");
    }

    #[test]
    fn glossary_parse_va_khop_ca_cau() {
        let g = Glossary::parse("# note\nLos Santos = Los Santos\nVice City = Vice City\nCops = Cảnh sát\n");
        assert_eq!(g.len(), 3);
        assert_eq!(g.exact("los santos"), Some("Los Santos"));
        assert_eq!(g.exact("Cops!"), Some("Cảnh sát"));
        assert_eq!(g.exact("Cops are here"), None);
    }

    #[test]
    fn protect_thay_ten_rieng_dung_nguyen_tu() {
        let g = Glossary::parse("Vice City = Vice City\nLos Santos = Los Santos\nSan = X\n");
        let (m, r) = g.protect("Leave vice city for Los Santos, not Santos or SANDY.");
        assert_eq!(r.len(), 2);
        assert!(!m.to_lowercase().contains("vice city"));
        assert!(!m.contains("Los Santos"));
        assert!(m.contains("not Santos or SANDY"));
    }

    #[test]
    fn find_word_chi_khop_nguyen_tu() {
        assert_eq!(find_word("a Vice Cityx b", "vice city"), None);
        assert_eq!(find_word("a Vice City b", "vice city"), Some(2));
    }

    #[test]
    fn cache_day_thi_bo_cai_cu_nhat() {
        let mut c = Cache::new(3);
        for k in ["a", "b", "c", "d"] {
            c.put(k.into(), k.to_uppercase());
        }
        assert!(c.get("a").is_none());
        assert_eq!(c.get("d").map(String::as_str), Some("D"));
        c.put("d".into(), "D2".into());
        assert_eq!(c.map.len(), 3);
        assert_eq!(c.get("d").map(String::as_str), Some("D2"));
    }

    /// Chay model that (bo qua neu chua tai model vao models/).
    #[test]
    fn dich_that_giu_ten_rieng_va_cache() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join(DEFAULT_MODEL_DIR);
        if !dir.exists() {
            eprintln!("bo qua: chua co model");
            return;
        }
        let mut t = Translator::load(&dir, 0).unwrap();
        let a = t.translate("We need to leave Vice City before the cops show up.").unwrap();
        assert!(!a.cached);
        assert!(a.text.contains("Vice City"), "{}", a.text);
        assert!(!a.text.contains("<unk>"));
        let b = t.translate("  We need to leave Vice City
before the cops show up. ").unwrap();
        assert!(b.cached);
        assert_eq!(a.text, b.text);
        assert_eq!(t.translate("Los Santos").unwrap().text, "Los Santos");
        assert_eq!(t.translate("   ").unwrap().text, "");
    }
}
