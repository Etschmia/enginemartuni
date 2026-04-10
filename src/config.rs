use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

pub struct Config {
    pub hash_size_mb: usize,
    pub book_dir: PathBuf,
    pub book_files: Vec<String>,
}

impl Config {
    pub fn load() -> Self {
        let candidates = env_candidates();
        let (env_map, base_dir, source) = find_and_parse_env(&candidates);

        match source {
            Some(p) => println!("info string config loaded from {}", p.display()),
            None => println!("info string config: no .env found, using defaults"),
        }

        let hash_size_mb = env_map
            .get("HASH_SIZE_MB")
            .and_then(|v| v.parse::<usize>().ok())
            .map(|v| v.max(1))
            .unwrap_or(64);

        let book_dir = {
            let raw = env_map
                .get("BOOK_DIR")
                .cloned()
                .unwrap_or_else(|| "src/polyglot".to_string());
            resolve_path(&PathBuf::from(raw), &base_dir)
        };

        let book_files = env_map
            .get("BOOK_FILES")
            .map(|v| {
                v.split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
            })
            .filter(|v: &Vec<String>| !v.is_empty())
            .unwrap_or_else(|| {
                vec![
                    "gm2001.bin".to_string(),
                    "komodo.bin".to_string(),
                    "rodent.bin".to_string(),
                ]
            });

        Config {
            hash_size_mb,
            book_dir,
            book_files,
        }
    }
}

fn resolve_path(path: &Path, base: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}

/// Kandidaten fuer .env-Suche und Base-Directory relativer Pfade:
/// 1) aktuelles Arbeitsverzeichnis
/// 2) Verzeichnis der Binary selbst
/// 3) zwei Ebenen darueber (typischer Projekt-Root bei target/{debug,release}/)
fn env_candidates() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    let push_unique = |p: PathBuf, out: &mut Vec<PathBuf>| {
        if !out.contains(&p) {
            out.push(p);
        }
    };

    if let Ok(cwd) = env::current_dir() {
        push_unique(cwd, &mut out);
    }
    if let Ok(exe) = env::current_exe() {
        if let Some(dir) = exe.parent() {
            if let Ok(c) = dir.canonicalize() {
                push_unique(c, &mut out);
            } else {
                push_unique(dir.to_path_buf(), &mut out);
            }
            let root = dir.join("..").join("..");
            if let Ok(c) = root.canonicalize() {
                push_unique(c, &mut out);
            }
        }
    }
    out
}

fn find_and_parse_env(
    candidates: &[PathBuf],
) -> (HashMap<String, String>, PathBuf, Option<PathBuf>) {
    for dir in candidates {
        let path = dir.join(".env");
        if let Ok(content) = fs::read_to_string(&path) {
            return (parse_env(&content), dir.clone(), Some(path));
        }
    }
    // Kein .env gefunden — nimm den erstbesten Kandidaten, der nach
    // Projekt-Root aussieht (enthaelt src/polyglot), sonst CWD.
    let base = candidates
        .iter()
        .find(|d| d.join("src/polyglot").exists())
        .cloned()
        .or_else(|| candidates.first().cloned())
        .unwrap_or_else(|| PathBuf::from("."));
    (HashMap::new(), base, None)
}

fn parse_env(content: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for raw in content.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let value = v.trim().trim_matches('"').trim_matches('\'').to_string();
        map.insert(k.trim().to_string(), value);
    }
    map
}
