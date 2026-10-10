//! Bounded, redacted input. Repository code is never executed.
use regex::{Captures, Regex};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{path::{Component, Path}, sync::LazyLock};

pub const MAX_FILE: u64 = 512_000;
pub const MAX_REPO: usize = 12_000_000;
pub fn short(text: &str, count: usize) -> String { text.chars().take(count).collect() }
pub fn digest(text: &str) -> String { format!("{:x}", Sha256::digest(text.as_bytes())) }
pub fn safe_relative(file: &str) -> bool {
    !file.is_empty() && !file.contains('\\') && !Path::new(file).is_absolute()
        && !Path::new(file).components().any(|c| matches!(c, Component::ParentDir | Component::Prefix(_)))
}
pub fn allowed(file: &str) -> bool {
    let path=Path::new(file);
    let name=path.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
    let suffix=path.extension().unwrap_or_default().to_string_lossy().to_lowercase();
    safe_relative(file) && !path.components().any(|c| matches!(c.as_os_str().to_str(),Some(".git"|".wy"|".codex"|".claude"|".pi"|".opencode"|"node_modules"|"dist"|"build"|"target"|"vendor")))
        && ![".env","credential","secret","id_rsa","id_ed25519","auth.json"].iter().any(|w|name.contains(w))
        && ["ts","tsx","js","jsx","json","toml","yaml","yml","sql","md","txt","ini","cfg","go","rs","java"].contains(&suffix.as_str())
}
static REDACTIONS: LazyLock<Vec<Regex>> = LazyLock::new(|| [
    r"-----BEGIN [^-]*PRIVATE KEY-----[\s\S]*?-----END [^-]*PRIVATE KEY-----",
    r"\b(?:sk-[A-Za-z0-9_-]{12,}|gh[pousr]_[A-Za-z0-9_]{16,}|AKIA[A-Z0-9]{16})\b",
    r#"(?i)(\b(?:api[_-]?key|password|passwd|secret|access[_-]?token|authorization)\b["']?\s*[:=]\s*)(?:["'][^"'\n]*["']|[^\s,;}]+)"#,
    r"(?i)\bBearer\s+[A-Za-z0-9._~+/-]+=*", r"(\w+://)[^\s/@:]+:[^\s/@]+@",
    r"[\x00-\x08\x0b-\x1f\x7f]",
].iter().map(|r|Regex::new(r).unwrap()).collect());
pub fn redact(text: &str) -> String {
    let r=&*REDACTIONS;
    let text=r[0].replace_all(text, |c: &Captures|format!("[REDACTED PRIVATE KEY]{}", "\n".repeat(c[0].matches('\n').count())));
    let text=r[1].replace_all(&text,"[REDACTED]");
    let text=r[2].replace_all(&text,"${1}[REDACTED]");
    let text=r[3].replace_all(&text,"Bearer [REDACTED]");
    let text=r[4].replace_all(&text,"${1}[REDACTED]@");
    r[5].replace_all(&text,"").into_owned()
}
pub fn clean(v: &mut Value) {
    match v { Value::String(t)=>*t=redact(t),Value::Array(xs)=>xs.iter_mut().for_each(clean),Value::Object(xs)=>xs.values_mut().for_each(clean),_=>{} }
}
pub fn read_source(root: &Path, file: &str) -> Option<String> {
    if !allowed(file) {return None;}
    let mut path=root.to_path_buf();
    for part in Path::new(file).components() {path.push(part);if path.is_symlink(){return None;}}
    if !path.canonicalize().ok()?.starts_with(root.canonicalize().ok()?) {return None;}
    let meta=path.metadata().ok()?;
    if !meta.is_file() || meta.len()>MAX_FILE {return None;}
    let raw=std::fs::read(path).ok()?;
    if raw.contains(&0) {return None;}
    String::from_utf8(raw).ok()
}
