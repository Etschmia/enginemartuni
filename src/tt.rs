use chess::ChessMove;
use std::mem::size_of;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TtFlag {
    /// Leerer Slot — die TT hat fuer diesen Schluessel keinen Eintrag.
    Empty,
    /// Exakte Bewertung im Such-Window (alpha < score < beta).
    Exact,
    /// Lower Bound (Fail-High: score >= beta). Der wahre Wert kann hoeher
    /// liegen, ist aber mindestens `score`.
    Lower,
    /// Upper Bound (Fail-Low: score <= alpha). Der wahre Wert kann
    /// niedriger liegen, ist aber hoechstens `score`.
    Upper,
}

#[derive(Debug, Clone, Copy)]
pub struct TtEntry {
    pub key: u64,
    pub best_move: Option<ChessMove>,
    pub eval: i32,
    pub depth: i8,
    pub flag: TtFlag,
}

impl Default for TtEntry {
    fn default() -> Self {
        Self {
            key: 0,
            best_move: None,
            eval: 0,
            depth: -1,
            flag: TtFlag::Empty,
        }
    }
}

/// Transposition Table — reservierter RAM-Bereich fuer bereits bewertete
/// Stellungen. Hash-Slot pro Schluessel ueber Modulo, depth-/exact-preferred
/// Replacement (siehe `should_replace`). Wird aktiv von Alpha-Beta +
/// Quiescence in `search.rs` befuellt und in der Move-Ordering ueber den
/// Hash-Move genutzt. Groesse via UCI-Option `Hash` (MB), Default in
/// `config.rs`.
pub struct TranspositionTable {
    entries: Vec<TtEntry>,
    size_mb: usize,
}

impl TranspositionTable {
    pub fn new(size_mb: usize) -> Self {
        let num = Self::num_entries(size_mb);
        Self {
            entries: vec![TtEntry::default(); num],
            size_mb: size_mb.max(1),
        }
    }

    fn num_entries(size_mb: usize) -> usize {
        let bytes = size_mb.max(1) * 1024 * 1024;
        (bytes / size_of::<TtEntry>()).max(1)
    }

    pub fn clear(&mut self) {
        for e in self.entries.iter_mut() {
            *e = TtEntry::default();
        }
    }

    pub fn resize(&mut self, size_mb: usize) {
        let size_mb = size_mb.max(1);
        if size_mb == self.size_mb {
            self.clear();
            return;
        }
        self.size_mb = size_mb;
        self.entries = vec![TtEntry::default(); Self::num_entries(size_mb)];
    }

    pub fn size_mb(&self) -> usize {
        self.size_mb
    }

    pub fn probe(&self, key: u64) -> Option<&TtEntry> {
        let idx = (key as usize) % self.entries.len();
        let e = &self.entries[idx];
        if e.flag != TtFlag::Empty && e.key == key {
            Some(e)
        } else {
            None
        }
    }

    pub fn store(
        &mut self,
        key: u64,
        best_move: Option<ChessMove>,
        eval: i32,
        depth: i8,
        flag: TtFlag,
    ) {
        let idx = (key as usize) % self.entries.len();
        if !should_replace(self.entries[idx], key, depth, flag) {
            return;
        }
        self.entries[idx] = TtEntry {
            key,
            best_move,
            eval,
            depth,
            flag,
        };
    }
}

fn flag_priority(flag: TtFlag) -> i32 {
    match flag {
        TtFlag::Exact => 3,
        TtFlag::Lower | TtFlag::Upper => 2,
        TtFlag::Empty => 0,
    }
}

fn should_replace(old: TtEntry, new_key: u64, new_depth: i8, new_flag: TtFlag) -> bool {
    if old.flag == TtFlag::Empty {
        return true;
    }

    if old.key == new_key {
        if new_depth != old.depth {
            return new_depth > old.depth;
        }
        return flag_priority(new_flag) >= flag_priority(old.flag);
    }

    if new_depth > old.depth {
        return true;
    }
    if new_depth == old.depth {
        return flag_priority(new_flag) >= flag_priority(old.flag);
    }

    // Exact-Eintraege sind als Hash-Move und Score-Hinweis wertvoll genug, um
    // einen nur geringfuegig tieferen Bound bei Kollision zu verdraengen.
    new_flag == TtFlag::Exact && old.flag != TtFlag::Exact && new_depth + 2 >= old.depth
}

#[cfg(test)]
mod tests {
    use super::*;
    use chess::Square;

    fn mv() -> ChessMove {
        ChessMove::new(Square::A2, Square::A3, None)
    }

    fn one_slot_tt() -> TranspositionTable {
        TranspositionTable {
            entries: vec![TtEntry::default()],
            size_mb: 1,
        }
    }

    #[test]
    fn deeper_entry_survives_shallow_collision() {
        let mut tt = one_slot_tt();
        tt.store(1, Some(mv()), 40, 8, TtFlag::Lower);
        tt.store(2, None, 10, 4, TtFlag::Exact);

        let entry = tt.probe(1).expect("tiefer Eintrag bleibt erhalten");
        assert_eq!(entry.eval, 40);
        assert_eq!(entry.depth, 8);
        assert!(tt.probe(2).is_none());
    }

    #[test]
    fn exact_replaces_same_depth_bound() {
        let mut tt = one_slot_tt();
        tt.store(1, None, 10, 5, TtFlag::Lower);
        tt.store(1, Some(mv()), 22, 5, TtFlag::Exact);

        let entry = tt.probe(1).expect("exact ersetzt bound gleicher Tiefe");
        assert_eq!(entry.eval, 22);
        assert_eq!(entry.flag, TtFlag::Exact);
    }
}
