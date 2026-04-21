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
    /// Freibauern-Bonus nach Vormarsch-Rang (Index 0 = Ausgangsreihe, 5 = kurz vor Umwandlung).
    /// Kleiner Wert im Mittelspiel (leicht blockierbar), grosser Wert im Endspiel.
    pub pawn_passed_rank_bonuses: Vec<i32>,

    // Piece bonuses/penalties
    pub knight_backrank_penalty: i32,
    pub bishop_pair_each: i32,
    pub connected_rooks_pair: i32,
    /// Turm auf vollständig offener Linie (keine eigenen und keine gegnerischen Bauern)
    pub rook_open_file_bonus: i32,
    /// Turm auf halb-offener Linie (keine eigenen, aber gegnerische Bauern)
    pub rook_semiopen_file_bonus: i32,

    // King safety
    pub ks_knight_weight: i32,
    pub ks_bishop_weight: i32,
    pub ks_rook_weight: i32,
    pub ks_queen_weight: i32,
    pub ks_shield_rank1_bonus: i32,
    pub ks_shield_rank2_bonus: i32,
    pub ks_shield_missing_penalty: i32,
    pub ks_exposed_center_penalty: i32,
    pub safety_table: Vec<i32>,

    // Endspiel-Mop-up
    pub eg_corner_weight: i32,
    pub eg_king_proximity_weight: i32,
    pub eg_passed_unstoppable_bonus: i32,
    /// Bonus pro Zentralisierungseinheit für den aktiven Endspielkönig.
    /// Skaliert mit (threshold - phase) / threshold, wirkt nur unterhalb threshold.
    pub king_activity_bonus: i32,
    /// Phase-Schwelle (0..24), unterhalb derer König-Aktivität bewertet wird.
    pub king_activity_phase_threshold: i32,

    // Mobility (cp pro "safe" Zielfeld, getaperter Mittel-/Endspielbeitrag).
    // "Safe" = nicht eigene Figur und nicht von gegnerischem Bauern angegriffen.
    pub knight_mg_mobility: i32,
    pub knight_eg_mobility: i32,
    pub bishop_mg_mobility: i32,
    pub bishop_eg_mobility: i32,
    pub rook_mg_mobility: i32,
    pub rook_eg_mobility: i32,
    pub queen_mg_mobility: i32,
    pub queen_eg_mobility: i32,
}

pub const DEFAULT_SAFETY_TABLE: [i32; 100] = [
    0, 0, 1, 2, 3, 5, 7, 9, 12, 15, 18, 22, 26, 30, 35, 39, 44, 50, 56, 62, 68, 75, 82, 85, 89, 97,
    105, 113, 122, 131, 140, 150, 169, 180, 191, 202, 213, 225, 237, 248, 260, 272, 283, 295, 307,
    319, 330, 342, 354, 366, 377, 389, 401, 412, 424, 436, 448, 459, 471, 483, 494, 500, 500, 500,
    500, 500, 500, 500, 500, 500, 500, 500, 500, 500, 500, 500, 500, 500, 500, 500, 500, 500, 500,
    500, 500, 500, 500, 500, 500, 500, 500, 500, 500, 500, 500, 500, 500, 500, 500, 500,
];

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
            // Rank 0 (Ausgangsreihe) bis Rank 5 (eine Reihe vor Umwandlung).
            // Im Mittelspiel ist ein a2-Freibauer kaum gefährlich; ein a7-Freibauer ist es sehr.
            pawn_passed_rank_bonuses: vec![5, 15, 30, 55, 100, 170],

            knight_backrank_penalty: -50,
            bishop_pair_each: 15,
            connected_rooks_pair: 150,
            rook_open_file_bonus: 30,
            rook_semiopen_file_bonus: 15,

            ks_knight_weight: 2,
            ks_bishop_weight: 2,
            ks_rook_weight: 3,
            ks_queen_weight: 5,
            ks_shield_rank1_bonus: 10,
            ks_shield_rank2_bonus: 5,
            ks_shield_missing_penalty: -15,
            ks_exposed_center_penalty: -30,
            safety_table: DEFAULT_SAFETY_TABLE.to_vec(),

            eg_corner_weight: 20,
            eg_king_proximity_weight: 10,
            eg_passed_unstoppable_bonus: 500,
            king_activity_bonus: 3,
            king_activity_phase_threshold: 16,

            knight_mg_mobility: 3,
            knight_eg_mobility: 3,
            bishop_mg_mobility: 3,
            bishop_eg_mobility: 4,
            rook_mg_mobility: 2,
            rook_eg_mobility: 5,
            queen_mg_mobility: 1,
            queen_eg_mobility: 2,
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
        if let Some(arr) = pw
            .and_then(|s| s.get("passed_rank_bonuses"))
            .and_then(|v| v.as_array())
        {
            let parsed: Vec<i32> = arr
                .iter()
                .filter_map(|v| v.as_integer().map(|x| x as i32))
                .collect();
            if !parsed.is_empty() {
                p.pawn_passed_rank_bonuses = parsed;
            }
        }

        let pc = section(v, "pieces");
        p.knight_backrank_penalty = i(&pc, "knight_backrank_penalty", p.knight_backrank_penalty);
        p.bishop_pair_each = i(&pc, "bishop_pair_each", p.bishop_pair_each);
        p.connected_rooks_pair = i(&pc, "connected_rooks_pair", p.connected_rooks_pair);
        p.rook_open_file_bonus = i(&pc, "rook_open_file_bonus", p.rook_open_file_bonus);
        p.rook_semiopen_file_bonus = i(&pc, "rook_semiopen_file_bonus", p.rook_semiopen_file_bonus);

        let ks = section(v, "king_safety");
        p.ks_knight_weight = i(&ks, "knight_weight", p.ks_knight_weight);
        p.ks_bishop_weight = i(&ks, "bishop_weight", p.ks_bishop_weight);
        p.ks_rook_weight = i(&ks, "rook_weight", p.ks_rook_weight);
        p.ks_queen_weight = i(&ks, "queen_weight", p.ks_queen_weight);
        p.ks_shield_rank1_bonus = i(&ks, "shield_rank1_bonus", p.ks_shield_rank1_bonus);
        p.ks_shield_rank2_bonus = i(&ks, "shield_rank2_bonus", p.ks_shield_rank2_bonus);
        p.ks_shield_missing_penalty =
            i(&ks, "shield_missing_penalty", p.ks_shield_missing_penalty);
        p.ks_exposed_center_penalty =
            i(&ks, "exposed_center_penalty", p.ks_exposed_center_penalty);

        if let Some(arr) = ks
            .and_then(|s| s.get("safety_table"))
            .and_then(|v| v.as_array())
        {
            let parsed: Vec<i32> = arr
                .iter()
                .filter_map(|v| v.as_integer().map(|x| x as i32))
                .collect();
            if !parsed.is_empty() {
                p.safety_table = parsed;
            }
        }

        let eg = section(v, "endgame");
        p.eg_corner_weight = i(&eg, "corner_weight", p.eg_corner_weight);
        p.eg_king_proximity_weight =
            i(&eg, "king_proximity_weight", p.eg_king_proximity_weight);
        p.eg_passed_unstoppable_bonus = i(
            &eg,
            "passed_unstoppable_bonus",
            p.eg_passed_unstoppable_bonus,
        );
        p.king_activity_bonus = i(&eg, "king_activity_bonus", p.king_activity_bonus);
        p.king_activity_phase_threshold = i(
            &eg,
            "king_activity_phase_threshold",
            p.king_activity_phase_threshold,
        );

        let mob = section(v, "mobility");
        p.knight_mg_mobility = i(&mob, "knight_mg", p.knight_mg_mobility);
        p.knight_eg_mobility = i(&mob, "knight_eg", p.knight_eg_mobility);
        p.bishop_mg_mobility = i(&mob, "bishop_mg", p.bishop_mg_mobility);
        p.bishop_eg_mobility = i(&mob, "bishop_eg", p.bishop_eg_mobility);
        p.rook_mg_mobility = i(&mob, "rook_mg", p.rook_mg_mobility);
        p.rook_eg_mobility = i(&mob, "rook_eg", p.rook_eg_mobility);
        p.queen_mg_mobility = i(&mob, "queen_mg", p.queen_mg_mobility);
        p.queen_eg_mobility = i(&mob, "queen_eg", p.queen_eg_mobility);

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
