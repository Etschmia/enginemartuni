//! Hand-codiertes Endspielwissen ohne Tablebases.
//!
//! Klassifiziert die Stellung anhand der Material-Signatur und liefert
//! fuer bekannte Endspiele eine Spezial-Bewertung. Dadurch findet die
//! Engine elementare Matts, die mit purer Material-/PST-Eval nicht
//! zuverlaessig zu loesen sind.
//!
//! Konzept und Phasen siehe `docs/endgame.md`.
//!
//! Phase A: Mop-up fuer KRvK, KQvK, KRRvK, KQQvK.
//! Phase B: KPK mit Rule of the Square.
//! Phase C: KBNK mit laeuferfarbigen Mattecken.

use crate::eval_config::EvalParams;
use chess::{BitBoard, Board, Color, Piece, Rank, Square};

/// Erkennt eine bekannte Material-Signatur und liefert die Bewertung
/// in Centipawns aus Sicht von Weiss. `None` bedeutet "keine Spezialregel
/// zustaendig" — die normale Eval uebernimmt.
pub fn endgame_score(board: &Board, p: &EvalParams) -> Option<i32> {
    match signature(board)? {
        Signature::Mopup { strong } => Some(mop_up_score(board, strong, p)),
        Signature::Kpk { strong, pawn_sq } => kpk_score(board, strong, pawn_sq, p),
        Signature::Kbnk { strong, bishop_sq } => {
            Some(kbnk_score(board, strong, bishop_sq, p))
        }
    }
}

/// Cheap-Check fuer den Suchhebel: gilt die Stellung als bekanntes
/// Endspiel? Wird in der Suche genutzt, um Extensions zu vergeben.
pub fn is_recognized(board: &Board) -> bool {
    signature(board).is_some()
}

#[derive(Copy, Clone)]
enum Signature {
    /// KRvK, KQvK, KRRvK, KQQvK — alle Mop-up-Endspiele
    Mopup { strong: Color },
    Kpk { strong: Color, pawn_sq: Square },
    Kbnk { strong: Color, bishop_sq: Square },
}

fn signature(board: &Board) -> Option<Signature> {
    let w_pawns = count(board, Piece::Pawn, Color::White);
    let b_pawns = count(board, Piece::Pawn, Color::Black);
    let w_knight = count(board, Piece::Knight, Color::White);
    let b_knight = count(board, Piece::Knight, Color::Black);
    let w_bishop = count(board, Piece::Bishop, Color::White);
    let b_bishop = count(board, Piece::Bishop, Color::Black);
    let w_rook = count(board, Piece::Rook, Color::White);
    let b_rook = count(board, Piece::Rook, Color::Black);
    let w_queen = count(board, Piece::Queen, Color::White);
    let b_queen = count(board, Piece::Queen, Color::Black);

    let w_minor_or_more = w_knight + w_bishop + w_rook + w_queen;
    let b_minor_or_more = b_knight + b_bishop + b_rook + b_queen;

    // KPK: genau ein Bauer, keine sonstigen Figuren
    if w_pawns + b_pawns == 1 && w_minor_or_more == 0 && b_minor_or_more == 0 {
        let strong = if w_pawns == 1 { Color::White } else { Color::Black };
        let pawn_sq = first_square(
            *board.pieces(Piece::Pawn) & *board.color_combined(strong),
        )?;
        return Some(Signature::Kpk { strong, pawn_sq });
    }

    if w_pawns + b_pawns != 0 {
        return None;
    }

    // Mop-up: eine Seite ist nackt, die andere hat nur schwere Figuren
    if b_minor_or_more == 0 && is_mopup_force(w_knight, w_bishop, w_rook, w_queen) {
        return Some(Signature::Mopup { strong: Color::White });
    }
    if w_minor_or_more == 0 && is_mopup_force(b_knight, b_bishop, b_rook, b_queen) {
        return Some(Signature::Mopup { strong: Color::Black });
    }

    // KBNK: genau ein Laeufer + ein Springer auf der starken Seite, andere Seite nackt
    if b_minor_or_more == 0 && w_bishop == 1 && w_knight == 1 && w_rook + w_queen == 0 {
        let bishop_sq = first_square(
            *board.pieces(Piece::Bishop) & *board.color_combined(Color::White),
        )?;
        return Some(Signature::Kbnk {
            strong: Color::White,
            bishop_sq,
        });
    }
    if w_minor_or_more == 0 && b_bishop == 1 && b_knight == 1 && b_rook + b_queen == 0 {
        let bishop_sq = first_square(
            *board.pieces(Piece::Bishop) & *board.color_combined(Color::Black),
        )?;
        return Some(Signature::Kbnk {
            strong: Color::Black,
            bishop_sq,
        });
    }

    None
}

/// Mop-up trifft, wenn die starke Seite *nur* schwere Figuren hat (Turm, Dame)
/// und mindestens eine davon. Damit sind KRvK, KQvK, KRRvK, KQQvK abgedeckt.
fn is_mopup_force(n: u32, b: u32, r: u32, q: u32) -> bool {
    if n != 0 || b != 0 {
        return false;
    }
    r >= 1 || q >= 1
}

fn mop_up_score(board: &Board, strong: Color, p: &EvalParams) -> i32 {
    let weak = !strong;
    let weak_king = board.king_square(weak);
    let strong_king = board.king_square(strong);

    let corner_d = nearest_corner_distance(weak_king, &ALL_CORNERS);
    let king_d = chebyshev(weak_king, strong_king);

    let bonus = p.eg_corner_weight * (7 - corner_d)
        + p.eg_king_proximity_weight * (14 - 2 * king_d);

    let material = strong_material(board, strong, p);
    signed(material + bonus, strong)
}

/// Phase C: KBNK. Mattsetzen ist nur in einer Ecke der Laeuferfarbe
/// moeglich — der Mop-up-Gradient zieht den Verteidiger gezielt dorthin.
fn kbnk_score(board: &Board, strong: Color, bishop_sq: Square, p: &EvalParams) -> i32 {
    let weak = !strong;
    let weak_king = board.king_square(weak);
    let strong_king = board.king_square(strong);

    let corners: &[Square; 2] = if is_light_square(bishop_sq) {
        &LIGHT_CORNERS
    } else {
        &DARK_CORNERS
    };
    let corner_d = nearest_corner_distance(weak_king, corners);
    let king_d = chebyshev(weak_king, strong_king);

    let bonus = p.eg_corner_weight * (7 - corner_d)
        + p.eg_king_proximity_weight * (14 - 2 * king_d);

    let material = strong_material(board, strong, p);
    signed(material + bonus, strong)
}

fn is_light_square(sq: Square) -> bool {
    (sq.get_file().to_index() + sq.get_rank().to_index()) % 2 == 1
}

/// Phase B: Rule of the Square. Wenn der Bauer nicht mehr einholbar ist,
/// liefert die Funktion die Spezial-Bewertung. Sonst `None`, damit die
/// normale Eval die KPK-Stellung uebernimmt.
fn kpk_score(
    board: &Board,
    strong: Color,
    pawn_sq: Square,
    p: &EvalParams,
) -> Option<i32> {
    if is_pawn_unstoppable(board, strong, pawn_sq) {
        let cp = p.pawn + p.eg_passed_unstoppable_bonus;
        return Some(signed(cp, strong));
    }
    None
}

fn is_pawn_unstoppable(board: &Board, strong: Color, pawn_sq: Square) -> bool {
    let weak = !strong;
    let weak_king = board.king_square(weak);

    let promo_sq = promotion_square(pawn_sq, strong);
    let pawn_dist = pawn_distance_to_promotion(pawn_sq, strong);
    let king_dist = chebyshev(weak_king, promo_sq);

    // Verteidiger am Zug: er kommt rechtzeitig, wenn king_dist <= pawn_dist.
    // Angreifer am Zug: der Bauer macht den ersten Schritt, dem Verteidiger
    // fehlt eine Tempo-Einheit → king_dist <= pawn_dist - 1.
    let in_square = if board.side_to_move() == weak {
        king_dist <= pawn_dist
    } else {
        king_dist <= pawn_dist - 1
    };
    !in_square
}

fn promotion_square(pawn_sq: Square, color: Color) -> Square {
    let rank = match color {
        Color::White => Rank::Eighth,
        Color::Black => Rank::First,
    };
    Square::make_square(rank, pawn_sq.get_file())
}

/// Schritte, die der Bauer noch braucht, inkl. Doppelschritt von Reihe 2/7.
fn pawn_distance_to_promotion(pawn_sq: Square, color: Color) -> i32 {
    let r = pawn_sq.get_rank().to_index() as i32;
    match color {
        Color::White => {
            let d = 7 - r;
            if r == 1 { d - 1 } else { d }
        }
        Color::Black => {
            let d = r;
            if r == 6 { d - 1 } else { d }
        }
    }
}

fn first_square(bb: BitBoard) -> Option<Square> {
    bb.into_iter().next()
}

fn strong_material(board: &Board, strong: Color, p: &EvalParams) -> i32 {
    let bb = *board.color_combined(strong);
    let mut total = 0;
    total += (*board.pieces(Piece::Pawn) & bb).popcnt() as i32 * p.pawn;
    total += (*board.pieces(Piece::Knight) & bb).popcnt() as i32 * p.knight;
    total += (*board.pieces(Piece::Bishop) & bb).popcnt() as i32 * p.bishop;
    total += (*board.pieces(Piece::Rook) & bb).popcnt() as i32 * p.rook;
    total += (*board.pieces(Piece::Queen) & bb).popcnt() as i32 * p.queen;
    total
}

fn count(board: &Board, piece: Piece, color: Color) -> u32 {
    (*board.pieces(piece) & *board.color_combined(color)).popcnt()
}

fn signed(cp: i32, strong: Color) -> i32 {
    if strong == Color::White { cp } else { -cp }
}

fn chebyshev(a: Square, b: Square) -> i32 {
    let df = (a.get_file().to_index() as i32 - b.get_file().to_index() as i32).abs();
    let dr = (a.get_rank().to_index() as i32 - b.get_rank().to_index() as i32).abs();
    df.max(dr)
}

fn nearest_corner_distance(sq: Square, corners: &[Square]) -> i32 {
    corners
        .iter()
        .map(|c| chebyshev(sq, *c))
        .min()
        .unwrap_or(0)
}

const ALL_CORNERS: [Square; 4] =
    [Square::A1, Square::H1, Square::A8, Square::H8];

const LIGHT_CORNERS: [Square; 2] = [Square::A8, Square::H1];

const DARK_CORNERS: [Square; 2] = [Square::A1, Square::H8];

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn p() -> EvalParams {
        EvalParams::default()
    }

    #[test]
    fn signature_krvk() {
        let b = Board::from_str("4k3/8/8/8/8/8/8/4K2R w - - 0 1").unwrap();
        assert!(matches!(
            signature(&b),
            Some(Signature::Mopup { strong: Color::White })
        ));
    }

    #[test]
    fn signature_kqvk_black_strong() {
        let b = Board::from_str("4k3/8/8/8/8/8/8/3qK3 w - - 0 1").unwrap();
        assert!(matches!(
            signature(&b),
            Some(Signature::Mopup { strong: Color::Black })
        ));
    }

    #[test]
    fn signature_krrvk() {
        let b = Board::from_str("4k3/8/8/8/8/8/8/R3K2R w - - 0 1").unwrap();
        assert!(matches!(
            signature(&b),
            Some(Signature::Mopup { strong: Color::White })
        ));
    }

    #[test]
    fn signature_kbk_is_not_mopup() {
        // KBvK ist Remis — kein bekanntes Endspiel
        let b = Board::from_str("4k3/8/8/8/8/8/8/3BK3 w - - 0 1").unwrap();
        assert!(signature(&b).is_none());
    }

    #[test]
    fn signature_with_pawns_other_pieces_is_none() {
        let b = Board::from_str("4k3/8/8/8/8/3N4/4P3/4K3 w - - 0 1").unwrap();
        assert!(signature(&b).is_none());
    }

    #[test]
    fn mopup_drives_weak_king_to_corner() {
        // Beide Stellungen mit aehnlicher Koenigsdistanz → der Eckenterm
        // entscheidet. Schwarzer Koenig in der Ecke ist deutlich schlechter
        // (= besser fuer Weiss) als im Zentrum.
        let center =
            Board::from_str("8/8/8/3k4/8/4K3/8/7R w - - 0 1").unwrap();
        let edge =
            Board::from_str("k7/8/2K5/8/8/8/8/7R w - - 0 1").unwrap();
        let s_center = endgame_score(&center, &p()).unwrap();
        let s_edge = endgame_score(&edge, &p()).unwrap();
        assert!(s_edge > s_center, "edge {s_edge} should beat center {s_center}");
    }

    #[test]
    fn mopup_strong_king_proximity_helps() {
        // Beide Koenige nah → besserer Mop-up-Score als weit entfernt.
        let near = Board::from_str("k7/8/2K5/8/8/8/8/7R w - - 0 1").unwrap();
        let far = Board::from_str("k7/8/8/8/8/8/8/4K2R w - - 0 1").unwrap();
        let s_near = endgame_score(&near, &p()).unwrap();
        let s_far = endgame_score(&far, &p()).unwrap();
        assert!(s_near > s_far, "near {s_near} should beat far {s_far}");
    }

    #[test]
    fn signature_kpk() {
        let b = Board::from_str("4k3/8/8/8/8/8/4P3/4K3 w - - 0 1").unwrap();
        assert!(matches!(
            signature(&b),
            Some(Signature::Kpk { strong: Color::White, .. })
        ));
    }

    #[test]
    fn kpk_outside_square_is_unstoppable() {
        // Bauer auf a4, schwarzer Koenig in h8 — voellig ausserhalb.
        // Weiss am Zug.
        let b = Board::from_str("7k/8/8/8/P7/8/8/4K3 w - - 0 1").unwrap();
        let s = endgame_score(&b, &p()).unwrap();
        assert!(s >= p().eg_passed_unstoppable_bonus, "got {s}");
    }

    #[test]
    fn kpk_inside_square_returns_none() {
        // Schwarzer Koenig direkt vor dem Bauern — eindeutig im Quadrat.
        let b = Board::from_str("8/8/3k4/8/3P4/8/8/4K3 w - - 0 1").unwrap();
        assert!(endgame_score(&b, &p()).is_none());
    }

    #[test]
    fn signature_kbnk() {
        let b = Board::from_str("4k3/8/8/8/8/8/8/2BNK3 w - - 0 1").unwrap();
        assert!(matches!(
            signature(&b),
            Some(Signature::Kbnk { strong: Color::White, .. })
        ));
    }

    #[test]
    fn kbnk_drives_to_bishop_color_corner() {
        // Weisser Dunkelfeldlaeufer auf c1 → Mattecken sind a1 und h8.
        // Schwarzer Koenig auf h1 (dunkle Ecke) → guter Mop-up-Score.
        let dark_corner = Board::from_str("8/8/8/8/4K3/8/2N5/2B4k w - - 0 1").unwrap();
        // Schwarzer Koenig auf h8 (auch dunkle Ecke).
        let other_dark = Board::from_str("7k/8/8/8/4K3/8/2N5/2B5 w - - 0 1").unwrap();
        // Schwarzer Koenig auf a8 (helle Ecke) — schlechter, weil falsche Farbe.
        let light_corner = Board::from_str("k7/8/8/8/4K3/8/2N5/2B5 w - - 0 1").unwrap();

        let s_h1 = endgame_score(&dark_corner, &p()).unwrap();
        let s_h8 = endgame_score(&other_dark, &p()).unwrap();
        let s_a8 = endgame_score(&light_corner, &p()).unwrap();
        assert!(s_h1 > s_a8, "h1 {s_h1} should beat a8 {s_a8}");
        assert!(s_h8 > s_a8, "h8 {s_h8} should beat a8 {s_a8}");
    }

    #[test]
    fn light_dark_square_classification() {
        assert!(!is_light_square(Square::A1));
        assert!(is_light_square(Square::H1));
        assert!(is_light_square(Square::A8));
        assert!(!is_light_square(Square::H8));
    }
}
