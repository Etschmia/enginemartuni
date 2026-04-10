use chess::{Board, Color, File, Piece, Square, ALL_SQUARES};

use super::random::{
    POLYGLOT_RANDOM, RANDOM_CASTLE, RANDOM_EN_PASSANT, RANDOM_PIECE, RANDOM_TURN,
};

pub fn polyglot_hash(board: &Board) -> u64 {
    let mut key: u64 = 0;

    for &sq in ALL_SQUARES.iter() {
        if let Some(piece) = board.piece_on(sq) {
            let color = board.color_on(sq).unwrap();
            let kind = polyglot_piece_index(piece, color);
            let file = sq.get_file().to_index();
            let rank = sq.get_rank().to_index();
            key ^= POLYGLOT_RANDOM[RANDOM_PIECE + 64 * kind + 8 * rank + file];
        }
    }

    let w = board.castle_rights(Color::White);
    let b = board.castle_rights(Color::Black);
    if w.has_kingside() {
        key ^= POLYGLOT_RANDOM[RANDOM_CASTLE];
    }
    if w.has_queenside() {
        key ^= POLYGLOT_RANDOM[RANDOM_CASTLE + 1];
    }
    if b.has_kingside() {
        key ^= POLYGLOT_RANDOM[RANDOM_CASTLE + 2];
    }
    if b.has_queenside() {
        key ^= POLYGLOT_RANDOM[RANDOM_CASTLE + 3];
    }

    // En passant wird nur gemischt, wenn ein Bauer der ziehenden Seite
    // tatsaechlich en passant schlagen koennte.
    if let Some(ep_pawn_sq) = board.en_passant() {
        let stm = board.side_to_move();
        let ep_file = ep_pawn_sq.get_file().to_index();
        let ep_rank = ep_pawn_sq.get_rank();

        let mut can_capture = false;
        for df in [-1i32, 1] {
            let adj = ep_file as i32 + df;
            if !(0..8).contains(&adj) {
                continue;
            }
            let adj_sq = Square::make_square(ep_rank, File::from_index(adj as usize));
            if board.piece_on(adj_sq) == Some(Piece::Pawn)
                && board.color_on(adj_sq) == Some(stm)
            {
                can_capture = true;
                break;
            }
        }

        if can_capture {
            key ^= POLYGLOT_RANDOM[RANDOM_EN_PASSANT + ep_file];
        }
    }

    if board.side_to_move() == Color::White {
        key ^= POLYGLOT_RANDOM[RANDOM_TURN];
    }

    key
}

fn polyglot_piece_index(piece: Piece, color: Color) -> usize {
    let base = match piece {
        Piece::Pawn => 0,
        Piece::Knight => 2,
        Piece::Bishop => 4,
        Piece::Rook => 6,
        Piece::Queen => 8,
        Piece::King => 10,
    };
    base + if color == Color::White { 1 } else { 0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chess::Board;
    use std::str::FromStr;

    // Referenzwerte aus der Polyglot-Spezifikation.
    #[test]
    fn startpos() {
        let b = Board::default();
        assert_eq!(polyglot_hash(&b), 0x463b96181691fc9c);
    }

    #[test]
    fn after_e2e4() {
        let b =
            Board::from_str("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1").unwrap();
        assert_eq!(polyglot_hash(&b), 0x823c9b50fd114196);
    }

    #[test]
    fn after_e2e4_d7d5() {
        let b =
            Board::from_str("rnbqkbnr/ppp1pppp/8/3p4/4P3/8/PPPP1PPP/RNBQKBNR w KQkq d6 0 2").unwrap();
        assert_eq!(polyglot_hash(&b), 0x0756b94461c50fb0);
    }

    #[test]
    fn after_e2e4_d7d5_e4e5() {
        let b =
            Board::from_str("rnbqkbnr/ppp1pppp/8/3pP3/8/8/PPPP1PPP/RNBQKBNR b KQkq - 0 2").unwrap();
        assert_eq!(polyglot_hash(&b), 0x662fafb965db29d4);
    }

    #[test]
    fn after_e2e4_d7d5_e4e5_f7f5() {
        // En passant square aktiv UND ein weisser Bauer koennte schlagen (e5 auf f6)
        let b =
            Board::from_str("rnbqkbnr/ppp1p1pp/8/3pPp2/8/8/PPPP1PPP/RNBQKBNR w KQkq f6 0 3").unwrap();
        assert_eq!(polyglot_hash(&b), 0x22a48b5a8e47ff78);
    }
}
