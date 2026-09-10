//! Exact variant SEE material after playing on a raw infrastructure position.
//! No mirrored engine board, hash, outcome or child legal list is needed.
use shakmaty::{Move, Position, Role};

pub(crate) fn balance_after<P: Position + Clone>(pos: &P, mv: Move, values: [i32; 6]) -> i32 {
    let mover = pos.turn();
    let mut after = pos.clone();
    after.play_unchecked(mv);
    let material = |color| {
        [Role::Pawn, Role::Knight, Role::Bishop, Role::Rook, Role::Queen, Role::King]
            .iter().zip(values).map(|(&role, value)| {
                let on_board = (after.board().by_role(role) & after.board().by_color(color)).count();
                let pocket = after.pockets().map_or(0, |p| *p.get(color).get(role) as usize);
                (on_board + pocket) as i32 * value
            }).sum::<i32>()
    };
    material(mover) - material(!mover)
}
