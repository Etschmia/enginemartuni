use crate::eval_config::EvalParams;
use chess::{BitBoard, Board, Color, File, Piece, Rank, Square};

/// Stellungsbewertung in Centipawns, aus Sicht von Weiss.
/// Positiv = gut fuer Weiss, negativ = gut fuer Schwarz.
pub fn evaluate(board: &Board, p: &EvalParams) -> i32 {
    evaluate_side(board, Color::White, p) - evaluate_side(board, Color::Black, p)
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
        // Weisser Bauer auf a4 ohne Nachbar
        let b = Board::from_str("4k3/8/8/8/P7/8/8/4K3 w - - 0 1").unwrap();
        let p = EvalParams::default();
        // Erwartung: 100 (Bauer) + 0 (Linie a) - 20 (Isolani) + 300 (Freibauer) = 380
        assert_eq!(evaluate(&b, &p), 380);
    }

    #[test]
    fn phalanx_triple_and_de_bonus() {
        // Weisse Bauern auf d4, e4, f4 — alle Freibauern, e Linie bekommt 10, f und d jeweils
        let b = Board::from_str("4k3/8/8/8/3PPP2/8/8/4K3 w - - 0 1").unwrap();
        let p = EvalParams::default();
        // Erwartung: 3 * 100 (Material) = 300
        //   + d: de_bonus 10
        //   + e: de_bonus 10
        //   + f: cf_bonus  5
        //   + 3 * passed (alle passed) 900
        //   + phalanx_triple 30
        //   + alle 3 sind non-isolated
        // = 300 + 10 + 10 + 5 + 900 + 30 = 1255
        assert_eq!(evaluate(&b, &p), 1255);
    }

    #[test]
    fn bishop_pair_and_backrank_knight() {
        // Weiss hat Laeuferpaar, ein Springer auf b1
        let b = Board::from_str("4k3/8/8/8/8/8/8/1NB1KB2 w - - 0 1").unwrap();
        let p = EvalParams::default();
        // 2 Laeufer = 600 + pair bonus 2*15 = 630
        // 1 Springer = 300 - 50 (backrank) = 250
        // Summe: 880
        assert_eq!(evaluate(&b, &p), 880);
    }

    #[test]
    fn connected_rooks() {
        // Weisse Tuerme auf a1 und h1, Koenig auf e4 (ausserhalb der Reihe)
        let b = Board::from_str("3k4/8/8/8/4K3/8/8/R6R w - - 0 1").unwrap();
        let p = EvalParams::default();
        // 2 Tuerme = 1000 + connected 150 = 1150
        assert_eq!(evaluate(&b, &p), 1150);
    }

    #[test]
    fn rooks_not_connected_when_blocked() {
        // Laeufer auf d1 blockt die Verbindung zwischen a1 und h1
        let b = Board::from_str("3k4/8/8/8/4K3/8/8/R2B3R w - - 0 1").unwrap();
        let p = EvalParams::default();
        // 2 Tuerme 1000 + 1 Laeufer 300 (kein Pair) = 1300
        assert_eq!(evaluate(&b, &p), 1300);
    }
}
