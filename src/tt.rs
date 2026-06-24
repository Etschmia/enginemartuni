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
    /// Generation (Such-Alter), in der dieser Eintrag zuletzt geschrieben
    /// wurde. Wird in der Ersetzungsstrategie genutzt, um Fossilien aus
    /// frueheren Suchen (groesserer Generations-Abstand) leichter zu
    /// verdraengen. Passt ins vorhandene Alignment-Padding -> `size_of`
    /// unveraendert (kein Verlust an Eintraegen pro MB).
    pub generation: u8,
}

impl Default for TtEntry {
    fn default() -> Self {
        Self {
            key: 0,
            best_move: None,
            eval: 0,
            depth: -1,
            flag: TtFlag::Empty,
            generation: 0,
        }
    }
}

/// Tiefenstrafe pro Generations-Abstand in der Kollisions-Ersetzung
/// (Relevanz-Score, Stockfish-Stil): `relevanz = depth - PENALTY * alter`.
/// Ein um `n` Generationen veralteter Eintrag verliert `PENALTY * n`
/// effektive Tiefe und wird so von frischen Eintraegen verdraengt. Zentraler
/// Tuning-Knopf fuer das A/B (Start: 8, wie im Design abgenommen).
const GENERATION_AGE_PENALTY: i32 = 8;

/// Transposition Table — reservierter RAM-Bereich fuer bereits bewertete
/// Stellungen. Hash-Slot pro Schluessel ueber Modulo, depth-/exact-preferred
/// Replacement (siehe `should_replace`). Wird aktiv von Alpha-Beta +
/// Quiescence in `search.rs` befuellt und in der Move-Ordering ueber den
/// Hash-Move genutzt. Groesse via UCI-Option `Hash` (MB), Default in
/// `config.rs`.
pub struct TranspositionTable {
    entries: Vec<TtEntry>,
    size_mb: usize,
    /// Laufende Such-Generation. Wird zu Beginn JEDER Suche (`new_search`,
    /// einmal pro `go`) inkrementiert — NICHT bei `clear`. Neu gespeicherte
    /// Eintraege werden mit diesem Wert gestempelt; aeltere Eintraege gelten
    /// als veraltet (siehe `GENERATION_AGE_PENALTY`).
    generation: u8,
}

impl TranspositionTable {
    pub fn new(size_mb: usize) -> Self {
        let num = Self::num_entries(size_mb);
        Self {
            entries: vec![TtEntry::default(); num],
            size_mb: size_mb.max(1),
            generation: 0,
        }
    }

    /// Beginn einer neuen Suche: Generation hochzaehlen, damit Eintraege
    /// frueherer Suchen als veraltet erkannt werden. Wrappt bei u8-Ueberlauf
    /// (256 Generationen = 256 Zuege; so alte Eintraege sind laengst
    /// ueberschrieben, die Distanz-Arithmetik bleibt per `wrapping_sub`
    /// korrekt). Einmal pro `go` in `search()` aufrufen.
    pub fn new_search(&mut self) {
        self.generation = self.generation.wrapping_add(1);
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
        if !should_replace(self.entries[idx], key, depth, flag, self.generation) {
            return;
        }
        self.entries[idx] = TtEntry {
            key,
            best_move,
            eval,
            depth,
            flag,
            generation: self.generation,
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

fn should_replace(
    old: TtEntry,
    new_key: u64,
    new_depth: i8,
    new_flag: TtFlag,
    current_gen: u8,
) -> bool {
    if old.flag == TtFlag::Empty {
        return true;
    }

    if old.key == new_key {
        // Selbe Stellung: unveraendert depth-preferred. Derselbe Key liefert
        // denselben Stellungswert; ein tieferer alter Eintrag bleibt gueltig,
        // unabhaengig vom Alter — daher hier KEINE Generations-Abwertung.
        if new_depth != old.depth {
            return new_depth > old.depth;
        }
        return flag_priority(new_flag) >= flag_priority(old.flag);
    }

    // Kollision (anderer Key, selber Slot): den alten Eintrag um seine
    // Veralterung abwerten. Der neue Eintrag stammt immer aus der aktuellen
    // Suche (Alter 0), der alte evtl. aus einer frueheren. age == 0 (alle
    // Eintraege derselben Generation, z. B. innerhalb EINER frischen Suche)
    // -> `effective_old_depth == old.depth` -> die drei Zweige reduzieren sich
    // exakt auf die fruehere depth-preferred Baseline (bit-exakt verifiziert).
    let age = current_gen.wrapping_sub(old.generation) as i32;
    let effective_old_depth = old.depth as i32 - GENERATION_AGE_PENALTY * age;
    let new_depth_i = new_depth as i32;

    if new_depth_i > effective_old_depth {
        return true;
    }
    if new_depth_i == effective_old_depth {
        return flag_priority(new_flag) >= flag_priority(old.flag);
    }

    // Exact-Eintraege sind als Hash-Move und Score-Hinweis wertvoll genug, um
    // einen nur geringfuegig tieferen Bound bei Kollision zu verdraengen
    // (bezogen auf die nominale alte Tiefe — Baseline-Regel).
    new_flag == TtFlag::Exact && old.flag != TtFlag::Exact && new_depth as i32 + 2 >= old.depth as i32
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
            generation: 0,
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
    fn entry_size_unchanged_by_generation_field() {
        // generation: u8 passt ins Alignment-Padding -> kein Verlust an
        // Eintraegen pro MB (size_of bleibt durch u64-key 8-Byte-aligned).
        assert!(size_of::<TtEntry>() <= 24, "TtEntry zu gross: {}", size_of::<TtEntry>());
    }

    #[test]
    fn same_generation_is_depth_preferred_baseline() {
        // Ohne new_search() sind alle Eintraege Generation 0 (age==0) ->
        // reines depth-preferred wie vor der Generation/Age-Aenderung.
        let mut tt = one_slot_tt();
        tt.store(1, Some(mv()), 40, 8, TtFlag::Lower);
        // Flacherer Fremd-Key derselben Generation verdraengt NICHT.
        tt.store(2, None, 10, 4, TtFlag::Exact);
        assert_eq!(tt.probe(1).map(|e| e.depth), Some(8));
    }

    #[test]
    fn stale_deep_fossil_is_evicted_by_fresh_shallow_entry() {
        // Tiefer Eintrag aus einer frueheren Suche; nach genug new_search()-
        // Inkrementen ueberwiegt die Alters-Strafe seine Tiefe, sodass ein
        // frischer flacher Eintrag den Slot uebernimmt.
        let mut tt = one_slot_tt();
        tt.store(1, Some(mv()), 40, 8, TtFlag::Lower); // gen 0, depth 8

        // Zwei Generationen weiter: effektive Tiefe 8 - 8*2 = -8.
        tt.new_search();
        tt.new_search();
        // Frischer flacher Fremd-Key (depth 1) schlaegt das Fossil.
        tt.store(2, None, 5, 1, TtFlag::Upper);

        assert!(tt.probe(1).is_none(), "veraltetes Fossil muss verdraengt sein");
        let entry = tt.probe(2).expect("frischer Eintrag belegt den Slot");
        assert_eq!(entry.depth, 1);
    }

    #[test]
    fn fresh_deep_fossil_still_survives_shallow_collision() {
        // Gegenprobe: ohne Alterung (nur 1 Generation Abstand, Strafe 8)
        // ueberlebt ein depth-8-Eintrag eine depth-1-Kollision weiterhin
        // (8 - 8*1 = 0 >= 1? nein -> new_depth 1 > 0 -> wuerde ersetzen).
        // Daher hier Generations-Abstand 0: klassisches depth-preferred.
        let mut tt = one_slot_tt();
        tt.new_search(); // gen 1
        tt.store(1, Some(mv()), 40, 8, TtFlag::Lower); // gen 1, depth 8
        tt.store(2, None, 5, 1, TtFlag::Upper); // gen 1, depth 1, age 0
        assert_eq!(tt.probe(1).map(|e| e.depth), Some(8), "frischer tiefer Eintrag bleibt");
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
