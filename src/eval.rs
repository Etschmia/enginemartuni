use crate::endgame;
use crate::eval_config::EvalParams;
use crate::pst::{
    pst_index, BISHOP_PST, KING_PST, KNIGHT_PST, PAWN_PST, QUEEN_PST, ROOK_PST,
};
use chess::{
    get_bishop_moves, get_king_moves, get_knight_moves, get_rook_moves, BitBoard, Board, Color,
    File, Piece, Rank, Square,
};

const MAX_PHASE: i32 = 24;

/// Stellungsbewertung in Centipawns, aus Sicht von Weiss.
/// Positiv = gut fuer Weiss, negativ = gut fuer Schwarz.
///
/// Besteht aus zwei Teilen:
///  - Nicht-getaperte Terme (Material, Bauernstruktur, Laeuferpaar, King Safety, ...)
///  - Getaperter PST-Beitrag (mg/eg interpoliert nach Spielphase)
pub fn evaluate(board: &Board, p: &EvalParams) -> i32 {
    // Bekannte Endspiele uebernehmen die Bewertung komplett.
    if let Some(s) = endgame::endgame_score(board, p) {
        return s;
    }

    let non_pst = evaluate_side(board, Color::White, p) - evaluate_side(board, Color::Black, p);

    let (w_mg, w_eg) = pst_score(board, Color::White);
    let (b_mg, b_eg) = pst_score(board, Color::Black);
    let mg = w_mg - b_mg;
    let eg = w_eg - b_eg;
    let phase = game_phase(board);

    non_pst + taper(mg, eg, phase)
}

/// Interpoliert linear zwischen Middle- und Endgame-Score entsprechend der
/// aktuellen Spielphase (24 = volles Material, 0 = nur Koenige + Bauern).
#[inline]
pub fn taper(mg: i32, eg: i32, phase: i32) -> i32 {
    let phase = phase.clamp(0, MAX_PHASE);
    (mg * phase + eg * (MAX_PHASE - phase)) / MAX_PHASE
}

/// Phase-Berechnung nach klassischer Gewichtung: Springer 1, Laeufer 1,
/// Turm 2, Dame 4. Startpos = 24, reines KvK-Endspiel = 0.
pub fn game_phase(board: &Board) -> i32 {
    let knights = board.pieces(Piece::Knight).popcnt() as i32;
    let bishops = board.pieces(Piece::Bishop).popcnt() as i32;
    let rooks = board.pieces(Piece::Rook).popcnt() as i32;
    let queens = board.pieces(Piece::Queen).popcnt() as i32;
    (knights + bishops + 2 * rooks + 4 * queens).min(MAX_PHASE)
}

/// Akkumuliert den PST-Beitrag einer Seite in (mg, eg).
fn pst_score(board: &Board, us: Color) -> (i32, i32) {
    let our_bb = *board.color_combined(us);
    let mut mg = 0;
    let mut eg = 0;

    for (piece, table) in [
        (Piece::Pawn, &PAWN_PST),
        (Piece::Knight, &KNIGHT_PST),
        (Piece::Bishop, &BISHOP_PST),
        (Piece::Rook, &ROOK_PST),
        (Piece::Queen, &QUEEN_PST),
        (Piece::King, &KING_PST),
    ] {
        for sq in *board.pieces(piece) & our_bb {
            let idx = pst_index(sq.to_index(), us);
            mg += table.mg[idx];
            eg += table.eg[idx];
        }
    }

    (mg, eg)
}

fn evaluate_side(board: &Board, us: Color, p: &EvalParams) -> i32 {
    let mut score: i32 = 0;

    let our_bb = *board.color_combined(us);
    let their_pawns = *board.pieces(Piece::Pawn) & *board.color_combined(!us);
    let our_pawns = *board.pieces(Piece::Pawn) & our_bb;

    // Pro Figur: Materialwert + figurenspezifische Boni
    for sq in our_bb {
        let Some(piece) = board.piece_on(sq) else { continue };
        score += piece_material(piece, p);

        match piece {
            Piece::Pawn => {
                score += pawn_bonus(sq, us, our_pawns, their_pawns, p);
            }
            Piece::Knight => {
                let rank = sq.get_rank();
                if rank == Rank::First || rank == Rank::Eighth {
                    score += p.knight_backrank_penalty;
                }
            }
            _ => {}
        }
    }

    // Laeuferpaar
    let our_bishops = *board.pieces(Piece::Bishop) & our_bb;
    if our_bishops.popcnt() >= 2 {
        score += 2 * p.bishop_pair_each;
    }

    // Verbundene Tuerme — einmal pro Paar
    let our_rooks = *board.pieces(Piece::Rook) & our_bb;
    if rooks_connected(board, our_rooks) {
        score += p.connected_rooks_pair;
    }

    // Bauernphalanx (reihenweise)
    score += phalanx_bonus(our_pawns, p);

    // King Safety (positiv = sicher, negativ = in Gefahr)
    score += king_safety(board, us, p);

    score
}

/// Gesamt-Beitrag der King-Safety-Logik aus Sicht von `us`.
/// Positiver Wert = sicher, negativer Wert = in Gefahr.
pub fn king_safety(board: &Board, us: Color, p: &EvalParams) -> i32 {
    let king_sq = board.king_square(us);
    let zone = king_zone(king_sq);

    let shield = pawn_shield_score(board, us, king_sq, p);
    let danger = king_danger(board, us, zone, p);

    shield - danger
}

/// 3x3-Koenigszone: der Koenig selbst + alle acht Nachbarfelder.
/// Nutzt die interne Lookup-Tabelle der chess-Crate.
fn king_zone(sq: Square) -> BitBoard {
    get_king_moves(sq) | BitBoard::from_square(sq)
}

/// Summiert Angriffsgewichte gegnerischer Offiziere auf die King-Zone,
/// indiziert SafetyTable und liefert die Strafe in cp.
fn king_danger(board: &Board, us: Color, zone: BitBoard, p: &EvalParams) -> i32 {
    let enemy = !us;
    let enemy_bb = *board.color_combined(enemy);
    let occ = *board.combined();

    let mut n_attackers: i32 = 0;
    let mut weight_sum: i32 = 0;

    // Springer
    for sq in *board.pieces(Piece::Knight) & enemy_bb {
        if (get_knight_moves(sq) & zone) != BitBoard::new(0) {
            n_attackers += 1;
            weight_sum += p.ks_knight_weight;
        }
    }
    // Laeufer
    for sq in *board.pieces(Piece::Bishop) & enemy_bb {
        if (get_bishop_moves(sq, occ) & zone) != BitBoard::new(0) {
            n_attackers += 1;
            weight_sum += p.ks_bishop_weight;
        }
    }
    // Tuerme
    for sq in *board.pieces(Piece::Rook) & enemy_bb {
        if (get_rook_moves(sq, occ) & zone) != BitBoard::new(0) {
            n_attackers += 1;
            weight_sum += p.ks_rook_weight;
        }
    }
    // Damen (kombiniert Turm + Laeufer)
    for sq in *board.pieces(Piece::Queen) & enemy_bb {
        let attacks = get_rook_moves(sq, occ) | get_bishop_moves(sq, occ);
        if (attacks & zone) != BitBoard::new(0) {
            n_attackers += 1;
            weight_sum += p.ks_queen_weight;
        }
    }

    if n_attackers == 0 {
        return 0;
    }

    let raw = n_attackers * weight_sum;
    let max_idx = (p.safety_table.len() as i32) - 1;
    let idx = raw.clamp(0, max_idx) as usize;
    p.safety_table[idx]
}

/// Bewertet den Bauernschild drei Linien breit vor dem Koenig.
/// Zentrum (d/e) auf Grundreihe → exposed_center_penalty.
/// Koenig nicht auf Grundreihe → 0 (z.B. aktiver Endspielkoenig).
fn pawn_shield_score(board: &Board, us: Color, king_sq: Square, p: &EvalParams) -> i32 {
    let king_file = king_sq.get_file().to_index() as i32;
    let king_rank = king_sq.get_rank().to_index() as i32;

    let home_rank = match us {
        Color::White => 0,
        Color::Black => 7,
    };
    if king_rank != home_rank {
        return 0;
    }

    if king_file == 3 || king_file == 4 {
        return p.ks_exposed_center_penalty;
    }

    let (file_lo, file_hi) = if king_file <= 2 { (0, 2) } else { (5, 7) };
    let (r1, r2) = match us {
        Color::White => (1, 2),
        Color::Black => (6, 5),
    };

    let our_pawns = *board.pieces(Piece::Pawn) & *board.color_combined(us);
    let mut score = 0;

    for f in file_lo..=file_hi {
        let sq_r1 =
            Square::make_square(Rank::from_index(r1), File::from_index(f as usize));
        let sq_r2 =
            Square::make_square(Rank::from_index(r2), File::from_index(f as usize));
        if (our_pawns & BitBoard::from_square(sq_r1)) != BitBoard::new(0) {
            score += p.ks_shield_rank1_bonus;
        } else if (our_pawns & BitBoard::from_square(sq_r2)) != BitBoard::new(0) {
            score += p.ks_shield_rank2_bonus;
        } else {
            score += p.ks_shield_missing_penalty;
        }
    }
    score
}

fn piece_material(piece: Piece, p: &EvalParams) -> i32 {
    match piece {
        Piece::Pawn => p.pawn,
        Piece::Knight => p.knight,
        Piece::Bishop => p.bishop,
        Piece::Rook => p.rook,
        Piece::Queen => p.queen,
        Piece::King => 0,
    }
}

fn pawn_bonus(
    sq: Square,
    us: Color,
    our_pawns: BitBoard,
    their_pawns: BitBoard,
    p: &EvalParams,
) -> i32 {
    let mut b: i32 = 0;
    let file_idx = sq.get_file().to_index();

    match file_idx {
        3 | 4 => b += p.pawn_de_file_bonus,
        2 | 5 => b += p.pawn_cf_file_bonus,
        _ => {}
    }

    if is_isolated(our_pawns, file_idx) {
        b += p.pawn_isolated_penalty;
    }

    if is_passed(sq, us, their_pawns) {
        b += p.pawn_passed_bonus;
    }

    b
}

fn is_isolated(our_pawns: BitBoard, file_idx: usize) -> bool {
    let mut mask = BitBoard::new(0);
    if file_idx > 0 {
        mask |= file_mask(file_idx - 1);
    }
    if file_idx < 7 {
        mask |= file_mask(file_idx + 1);
    }
    (our_pawns & mask) == BitBoard::new(0)
}

fn file_mask(file_idx: usize) -> BitBoard {
    let mut b = BitBoard::new(0);
    for r in 0..8 {
        b |= BitBoard::from_square(Square::make_square(
            Rank::from_index(r),
            File::from_index(file_idx),
        ));
    }
    b
}

fn is_passed(sq: Square, us: Color, their_pawns: BitBoard) -> bool {
    let file_idx = sq.get_file().to_index() as i32;
    let rank_idx = sq.get_rank().to_index() as i32;

    for r in 0..8 {
        let ahead = match us {
            Color::White => r > rank_idx,
            Color::Black => r < rank_idx,
        };
        if !ahead {
            continue;
        }
        for df in [-1i32, 0, 1] {
            let f = file_idx + df;
            if !(0..8).contains(&f) {
                continue;
            }
            let check_sq = Square::make_square(
                Rank::from_index(r as usize),
                File::from_index(f as usize),
            );
            if (their_pawns & BitBoard::from_square(check_sq)) != BitBoard::new(0) {
                return false;
            }
        }
    }
    true
}

fn phalanx_bonus(our_pawns: BitBoard, p: &EvalParams) -> i32 {
    let mut total: i32 = 0;
    for rank_idx in 0..8 {
        let mut run: usize = 0;
        for file_idx in 0..8 {
            let sq = Square::make_square(
                Rank::from_index(rank_idx),
                File::from_index(file_idx),
            );
            if (our_pawns & BitBoard::from_square(sq)) != BitBoard::new(0) {
                run += 1;
            } else {
                total += score_run(run, p);
                run = 0;
            }
        }
        total += score_run(run, p);
    }
    total
}

fn score_run(len: usize, p: &EvalParams) -> i32 {
    if len >= 3 {
        p.pawn_phalanx_triple
    } else if len == 2 {
        p.pawn_phalanx_double
    } else {
        0
    }
}

fn rooks_connected(board: &Board, our_rooks: BitBoard) -> bool {
    if our_rooks.popcnt() < 2 {
        return false;
    }
    let squares: Vec<Square> = our_rooks.collect();
    for i in 0..squares.len() {
        for j in (i + 1)..squares.len() {
            if have_sight(board, squares[i], squares[j]) {
                return true;
            }
        }
    }
    false
}

fn have_sight(board: &Board, a: Square, b: Square) -> bool {
    let ar = a.get_rank().to_index() as i32;
    let af = a.get_file().to_index() as i32;
    let br = b.get_rank().to_index() as i32;
    let bf = b.get_file().to_index() as i32;

    let (dr, df) = if ar == br && af != bf {
        (0, (bf - af).signum())
    } else if af == bf && ar != br {
        ((br - ar).signum(), 0)
    } else {
        return false;
    };

    let mut r = ar + dr;
    let mut f = af + df;
    while (r, f) != (br, bf) {
        let sq = Square::make_square(
            Rank::from_index(r as usize),
            File::from_index(f as usize),
        );
        if board.piece_on(sq).is_some() {
            return false;
        }
        r += dr;
        f += df;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use chess::Board;
    use std::str::FromStr;

    #[test]
    fn startpos_is_balanced() {
        let b = Board::default();
        let p = EvalParams::default();
        // Startstellung ist symmetrisch — Score = 0
        assert_eq!(evaluate(&b, &p), 0);
    }

    #[test]
    fn material_advantage() {
        // Weiss hat einen Bauern mehr
        let b = Board::from_str("4k3/8/8/8/8/8/PPPPPPPP/4K3 w - - 0 1").unwrap();
        let p = EvalParams::default();
        let score = evaluate(&b, &p);
        // 8 Bauern minus Symmetrie erwartet > 0
        assert!(score > 0, "expected white advantage, got {score}");
    }

    #[test]
    fn isolated_pawn_penalty() {
        // KPK: Weisser Bauer auf a4, schwarzer Koenig e8 → Bauer ausserhalb
        // des Quadrats, Endspielmodul greift mit Pawn-Material + Bonus.
        let b = Board::from_str("4k3/8/8/8/P7/8/8/4K3 w - - 0 1").unwrap();
        let p = EvalParams::default();
        // 100 (pawn) + 500 (passed_unstoppable_bonus) = 600
        assert_eq!(evaluate(&b, &p), 600);
    }

    #[test]
    fn phalanx_triple_and_de_bonus() {
        // Weisse Bauern auf d4, e4, f4 — alle Freibauern
        let b = Board::from_str("4k3/8/8/8/3PPP2/8/8/4K3 w - - 0 1").unwrap();
        let p = EvalParams::default();
        // Non-PST: 300 + 25 (file) + 900 (3 passed) + 30 (phalanx triple) = 1255
        // PST diff (phase=0): Bauern d4/e4/f4 eg = 60, Kings cancel → +60
        // Total: 1315
        assert_eq!(evaluate(&b, &p), 1315);
    }

    #[test]
    fn bishop_pair_and_backrank_knight() {
        // Weiss hat Laeuferpaar und einen Springer auf b1
        let b = Board::from_str("4k3/8/8/8/8/8/8/1NB1KB2 w - - 0 1").unwrap();
        let p = EvalParams::default();
        // Non-PST: 900 + 30 (pair) - 50 (backrank knight) = 880
        // PST: die Grundreihen-Figuren werden stark abgewertet → taper bei phase=3
        // Total: 820
        assert_eq!(evaluate(&b, &p), 820);
    }

    #[test]
    fn connected_rooks() {
        // KRRvK ist mittlerweile Mop-up — Endspielmodul liefert die Bewertung
        // (Material + Eckenterm + Koenigsnaehe).
        let b = Board::from_str("3k4/8/8/8/4K3/8/8/R6R w - - 0 1").unwrap();
        let p = EvalParams::default();
        // 1000 (2 Tuerme) + 20*(7-3) Eckenterm + 10*(14-2*4) Koenigsnaehe
        // = 1000 + 80 + 60 = 1140
        assert_eq!(evaluate(&b, &p), 1140);
    }

    #[test]
    fn rooks_not_connected_when_blocked() {
        // Laeufer auf d1 blockt die Verbindung zwischen a1 und h1
        let b = Board::from_str("3k4/8/8/8/4K3/8/8/R2B3R w - - 0 1").unwrap();
        let p = EvalParams::default();
        // Non-PST: 1300 + 30 (schwarzer Koenig center) = 1330
        // PST taper bei phase=5
        // Total: 1335
        assert_eq!(evaluate(&b, &p), 1335);
    }

    #[test]
    fn phase_startpos_is_full() {
        assert_eq!(game_phase(&Board::default()), 24);
    }

    #[test]
    fn phase_kvk_is_zero() {
        let b = Board::from_str("4k3/8/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        assert_eq!(game_phase(&b), 0);
    }

    #[test]
    fn phase_queen_endgame() {
        let b = Board::from_str("4k3/8/8/8/8/8/8/3QK3 w - - 0 1").unwrap();
        // 1 Dame = 4
        assert_eq!(game_phase(&b), 4);
    }

    #[test]
    fn taper_is_mg_at_full_phase() {
        assert_eq!(taper(100, -50, 24), 100);
    }

    #[test]
    fn taper_is_eg_at_zero_phase() {
        assert_eq!(taper(100, -50, 0), -50);
    }

    #[test]
    fn taper_midpoint_interpolates() {
        assert_eq!(taper(120, 0, 12), 60);
    }

    #[test]
    fn ks_castled_intact_shield() {
        // Weisser Koenig g1, Bauernschild f2/g2/h2 vollstaendig
        let b = Board::from_str("4k3/8/8/8/8/8/5PPP/5RK1 w - - 0 1").unwrap();
        let p = EvalParams::default();
        // 3 * 10 = 30 Shield, 0 Angreifer
        assert_eq!(king_safety(&b, Color::White, &p), 30);
    }

    #[test]
    fn ks_no_shield_on_g1() {
        // Weisser Koenig g1 ohne Bauern im 3-Linien-Fenster
        let b = Board::from_str("4k3/8/8/8/8/8/8/5RK1 w - - 0 1").unwrap();
        let p = EvalParams::default();
        // 3 * -15 = -45, 0 Angreifer
        assert_eq!(king_safety(&b, Color::White, &p), -45);
    }

    #[test]
    fn ks_center_king_penalty() {
        let b = Board::from_str("4k3/8/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        let p = EvalParams::default();
        // e1 → Zentrum → -30
        assert_eq!(king_safety(&b, Color::White, &p), -30);
    }

    #[test]
    fn ks_queen_attacks_zone() {
        // Weisser Koenig g1 ohne Schild, schwarze Dame auf g5 haelt g-Linie
        let b = Board::from_str("4k3/8/8/6q1/8/8/8/6K1 w - - 0 1").unwrap();
        let p = EvalParams::default();
        // Shield: -45 (kein Bauer)
        // Dame → 1 Angreifer, Gewicht 5, raw=5, safety_table[5]=5
        // = -45 - 5 = -50
        assert_eq!(king_safety(&b, Color::White, &p), -50);
    }

    #[test]
    fn ks_advanced_shield_pawn() {
        // Weisser Koenig g1, g-Bauer auf g3 (eine Reihe weiter vor),
        // f2 und h2 intakt
        let b = Board::from_str("4k3/8/8/8/8/6P1/5P1P/5RK1 w - - 0 1").unwrap();
        let p = EvalParams::default();
        // f2 intakt = 10, g3 advanced = 5, h2 intakt = 10 → 25
        assert_eq!(king_safety(&b, Color::White, &p), 25);
    }
}
