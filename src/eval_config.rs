use std::env;
use std::fs;
use std::path::PathBuf;
use toml::Value;

#[derive(Debug, Clone)]
pub struct EvalParams {
    // Material
    pub pawn: i32,
    pub knight: i32,
    pub bishop: i32,
    pub rook: i32,
    pub queen: i32,

    // Pawn bonuses/penalties
    pub pawn_isolated_penalty: i32,
    pub pawn_de_file_bonus: i32,
    pub pawn_cf_file_bonus: i32,
    pub pawn_phalanx_triple: i32,
    pub pawn_phalanx_double: i32,
    pub pawn_passed_bonus: i32,

    // Piece bonuses/penalties
    pub knight_backrank_penalty: i32,
    pub bishop_pair_each: i32,
    pub connected_rooks_pair: i32,
}

impl Default for EvalParams {
    fn default() -> Self {
        Self {
            pawn: 100,
            knight: 300,
            bishop: 300,
            rook: 500,
            queen: 900,

            pawn_isolated_penalty: -20,
            pawn_de_file_bonus: 10,
            pawn_cf_file_bonus: 5,
            pawn_phalanx_triple: 30,
            pawn_phalanx_double: 15,
            pawn_passed_bonus: 300,

            knight_backrank_penalty: -50,
            bishop_pair_each: 15,
            connected_rooks_pair: 150,
        }
    }
}

impl EvalParams {
    pub fn load() -> Self {
        let (content, source) = find_and_read_eval_toml();

        let Some(content) = content else {
            println!("info string eval: no eval.toml found, using defaults");
            return Self::default();
        };

        match content.parse::<Value>() {
            Ok(v) => {
                println!(
                    "info string eval loaded from {}",
                    source.map(|p| p.display().to_string()).unwrap_or_default()
                );
                Self::from_toml(&v)
            }
            Err(e) => {
                println!("info string eval: parse error in eval.toml ({e}), using defaults");
                Self::default()
            }
        }
    }

    fn from_toml(v: &Value) -> Self {
        let mut p = Self::default();

        let mat = section(v, "material");
        p.pawn = i(&mat, "pawn", p.pawn);
        p.knight = i(&mat, "knight", p.knight);
        p.bishop = i(&mat, "bishop", p.bishop);
        p.rook = i(&mat, "rook", p.rook);
        p.queen = i(&mat, "queen", p.queen);

        let pw = section(v, "pawns");
        p.pawn_isolated_penalty = i(&pw, "isolated_penalty", p.pawn_isolated_penalty);
        p.pawn_de_file_bonus = i(&pw, "de_file_bonus", p.pawn_de_file_bonus);
        p.pawn_cf_file_bonus = i(&pw, "cf_file_bonus", p.pawn_cf_file_bonus);
        p.pawn_phalanx_triple = i(&pw, "phalanx_triple", p.pawn_phalanx_triple);
        p.pawn_phalanx_double = i(&pw, "phalanx_double", p.pawn_phalanx_double);
        p.pawn_passed_bonus = i(&pw, "passed_bonus", p.pawn_passed_bonus);

        let pc = section(v, "pieces");
        p.knight_backrank_penalty = i(&pc, "knight_backrank_penalty", p.knight_backrank_penalty);
        p.bishop_pair_each = i(&pc, "bishop_pair_each", p.bishop_pair_each);
        p.connected_rooks_pair = i(&pc, "connected_rooks_pair", p.connected_rooks_pair);

        p
    }
}

fn section<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    v.get(key)
}

fn i(section: &Option<&Value>, key: &str, default: i32) -> i32 {
    section
        .and_then(|s| s.get(key))
        .and_then(|v| v.as_integer())
        .map(|x| x as i32)
        .unwrap_or(default)
}

/// Analog zum .env-Lookup: CWD, Binary-Verzeichnis, Projekt-Root.
fn find_and_read_eval_toml() -> (Option<String>, Option<PathBuf>) {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(cwd) = env::current_dir() {
        candidates.push(cwd);
    }
    if let Ok(exe) = env::current_exe() {
        if let Some(dir) = exe.parent() {
            if let Ok(c) = dir.canonicalize() {
                if !candidates.contains(&c) {
                    candidates.push(c);
                }
            }
            if let Ok(c) = dir.join("..").join("..").canonicalize() {
                if !candidates.contains(&c) {
                    candidates.push(c);
                }
            }
        }
    }
    for dir in &candidates {
        let path = dir.join("eval.toml");
        if let Ok(content) = fs::read_to_string(&path) {
            return (Some(content), Some(path));
        }
    }
    (None, None)
}
