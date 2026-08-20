use crate::backend::EngineBoard;
use crate::endgame;
use crate::eval_config::EvalParams;
use crate::pst::{pst_index, BISHOP_PST, KING_PST, KNIGHT_PST, PAWN_PST, QUEEN_PST, ROOK_PST};
use chess::{
    get_adjacent_files, get_bishop_moves, get_file, get_king_moves, get_knight_moves,
    get_rook_moves, BitBoard, Color, File, Piece, Rank, Square,
};

const MAX_PHASE: i32 = 24;

/// Stellungsbewertung in Centipawns, aus Sicht von Weiss.
/// Positiv = gut fuer Weiss, negativ = gut fuer Schwarz.
///
/// Besteht aus zwei Teilen:
///  - Nicht-getaperte Terme (Material, Bauernstruktur, Laeuferpaar, King Safety, ...)
///  - Getaperter PST-Beitrag (mg/eg interpoliert nach Spielphase)
pub fn evaluate<B: EngineBoard>(board: &B, p: &EvalParams) -> i32 {
    // Bekannte Endspiele uebernehmen die Bewertung komplett.
    if board.uses_standard_rules() {
        if let Some(s) = endgame::endgame_score(board, p) {
            return s;
        }
    }

    let (w_mg, w_eg) = pst_score(board, Color::White);
    let (b_mg, b_eg) = pst_score(board, Color::Black);
    let mg = w_mg - b_mg;
    let eg = w_eg - b_eg;
    let phase = game_phase(board);
    let non_pst =
        evaluate_side(board, Color::White, p, phase) - evaluate_side(board, Color::Black, p, phase);

    let king_act = king_activity_endgame(board, phase, p);
    let king_pass_syn = king_passed_pawn_synergy(board, phase, p);
    let pawn_eg_guard = pawn_endgame_guard(board, phase, p);

    let (w_mob_mg, w_mob_eg) = mobility_score(board, Color::White, p);
    let (b_mob_mg, b_mob_eg) = mobility_score(board, Color::Black, p);
    let mob = taper(w_mob_mg - b_mob_mg, w_mob_eg - b_mob_eg, phase);

    // Turm hinter eigenem Bauern eingemauert — nur im Endspiel Malus.
    // Im Mittelspiel ist der Bauer Teil des Pawn-Shields oder der Eröffnungs-
    // struktur und der Turm normal entwickelt; im Endspiel will er aktiv sein.
    let w_trap_eg = rook_trapped_endgame_malus(board, Color::White, p);
    let b_trap_eg = rook_trapped_endgame_malus(board, Color::Black, p);
    let trap = taper(0, w_trap_eg - b_trap_eg, phase);

    non_pst
        + taper(mg, eg, phase)
        + king_act
        + king_pass_syn
        + pawn_eg_guard
        + mob
        + trap
        + material_imbalance(board, phase, p)
        + material_deficit_damping(board, phase, p, w_eg, b_eg)
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
#[inline]
pub fn game_phase<B: EngineBoard>(board: &B) -> i32 {
    let knights = board.pieces(Piece::Knight).popcnt() as i32;
    let bishops = board.pieces(Piece::Bishop).popcnt() as i32;
    let rooks = board.pieces(Piece::Rook).popcnt() as i32;
    let queens = board.pieces(Piece::Queen).popcnt() as i32;
    (knights + bishops + 2 * rooks + 4 * queens).min(MAX_PHASE)
}

/// Material-Ungleichgewicht: zwei Leichtfiguren wiegen im Mittelspiel mehr als
/// Turm+Bauer — Wissen, das die reine Stueckwert-Summe nicht erfasst (Diagnose
/// 31.05.2026: Martuni bewertete Turm+Bauer ≈ Springer+Laeufer und lief so
/// willentlich in -300-Stellungen, z.B. nach Nxf7 Rxf7 Bxf7+ Qxf7). Loest nur
/// beim klassischen "2 Leichtfiguren vs Turm(+Bauer)"-Muster aus: eine Seite hat
/// >= 2 Leichtfiguren mehr UND >= 1 Turm weniger. Der Bonus geht an die
/// Leichtfiguren-Mehrheits-Seite, phase-getapert (MG > EG, da zwei Leichtfiguren
/// mit Damen am Brett gefaehrlicher sind). Rueckgabe in Weiss-Sicht (positiv =
/// gut fuer Weiss). Code-Default der Parameter = 0 → Term inaktiv.
fn material_imbalance<B: EngineBoard>(board: &B, phase: i32, p: &EvalParams) -> i32 {
    let white = *board.color_combined(Color::White);
    let black = *board.color_combined(Color::Black);
    let knights = *board.pieces(Piece::Knight);
    let bishops = *board.pieces(Piece::Bishop);
    let rooks = *board.pieces(Piece::Rook);

    let w_minor = ((knights & white).popcnt() + (bishops & white).popcnt()) as i32;
    let b_minor = ((knights & black).popcnt() + (bishops & black).popcnt()) as i32;
    let minor_diff = w_minor - b_minor;
    let rook_diff = (rooks & white).popcnt() as i32 - (rooks & black).popcnt() as i32;

    let bonus = taper(p.imbalance_two_minors_mg, p.imbalance_two_minors_eg, phase);
    if minor_diff >= 2 && rook_diff <= -1 {
        bonus // Weiss hat die Leichtfiguren-Mehrheit gegen Turm(+Bauer)
    } else if minor_diff <= -2 && rook_diff >= 1 {
        -bonus // Schwarz hat die Leichtfiguren-Mehrheit gegen Turm(+Bauer)
    } else {
        0
    }
}

/// Material-Defizit-Daempfung der "Kompensations"-Terme (Diagnose 06.06.2026,
/// Bxe4-Bug, docs/roadmap.md). Problem: opfert Martuni eine Leichtfigur fuer
/// einen Bauern, bewertet es die Folgestellung ~300 cp zu optimistisch, weil ein
/// vorgeschobener Freibauer (`pawn_bonus`) und gute Figurenfelder (`pst_eg`) den
/// Figurenverlust kaschieren — obwohl der Gegner eine Mehrfigur zum Blockieren
/// oder Schlagen hat. Gegenmittel: liegt eine Seite statisch um
/// >= `damp_deficit_threshold` cp zurueck, werden IHRE Kompensations-Boni
/// gekuerzt — der Freibauer-Vormarschbonus auf `damp_passed_pct` %, der positive
/// PST-eg-Ueberschuss auf `damp_pst_eg_pct` %.
///
/// Rueckgabe in Weiss-Sicht (cp), als additive Korrektur analog `material_imbalance`:
/// ein Abschlag bei Weiss zieht das Total runter, ein Abschlag bei Schwarz hoch.
/// `w_pst_eg`/`b_pst_eg` sind die bereits in `evaluate()` berechneten Roh-PST-eg-
/// Summen je Seite (kein zweites `pst_score`). Beide Prozentsaetze 100 (Default)
/// → Rueckgabe 0 → verhaltensgleich. Code-Default damit inaktiv.
fn material_deficit_damping<B: EngineBoard>(
    board: &B,
    phase: i32,
    p: &EvalParams,
    w_pst_eg: i32,
    b_pst_eg: i32,
) -> i32 {
    // Kein Abschlag konfiguriert → No-op (spart die Material-Zaehlung im Hot-Path).
    if p.damp_passed_pct >= 100 && p.damp_pst_eg_pct >= 100 {
        return 0;
    }

    // Statisches Material beider Seiten aus den Anker-Werten [material] (ohne
    // Koenig). Bewusst NICHT die dynamische, getaperte Figurenbewertung — die
    // Schwelle soll ein stabiler Materialzaehler sein, kein bewegliches Ziel.
    let material = |us: Color| -> i32 {
        let bb = *board.color_combined(us);
        (*board.pieces(Piece::Pawn) & bb).popcnt() as i32 * p.pawn
            + (*board.pieces(Piece::Knight) & bb).popcnt() as i32 * p.knight
            + (*board.pieces(Piece::Bishop) & bb).popcnt() as i32 * p.bishop
            + (*board.pieces(Piece::Rook) & bb).popcnt() as i32 * p.rook
            + (*board.pieces(Piece::Queen) & bb).popcnt() as i32 * p.queen
    };
    let w_mat = material(Color::White);
    let b_mat = material(Color::Black);

    // Abschlag (>= 0 cp), den eine materiell unterlegene Seite auf ihre
    // Kompensations-Terme bekommt. 0, wenn die Seite nicht (genug) zurueckliegt.
    let side_cut = |us: Color, pst_eg: i32| -> i32 {
        let deficit = if us == Color::White {
            b_mat - w_mat
        } else {
            w_mat - b_mat
        };
        if deficit < p.damp_deficit_threshold {
            return 0;
        }
        // (1) Freibauer-Vormarschbonus dieser Seite anteilig kuerzen.
        let passed = side_passed_bonus_sum(board, us, p);
        let passed_cut = passed * (100 - p.damp_passed_pct) / 100;
        // (2) Positiven PST-eg-Ueberschuss anteilig kuerzen. Getapert mit 0 im
        //     MG, sodass der Abschlag dieselbe Phasen-Gewichtung traegt wie der
        //     PST-eg-Beitrag selbst (`taper(mg, eg, phase)`).
        let pst_cut_eg = pst_eg.max(0) * (100 - p.damp_pst_eg_pct) / 100;
        let pst_cut = taper(0, pst_cut_eg, phase);
        passed_cut + pst_cut
    };

    -side_cut(Color::White, w_pst_eg) + side_cut(Color::Black, b_pst_eg)
}

/// Summe der Freibauer-Vormarschboni einer Seite. Spiegelt die Freibauer-Logik
/// aus `pawn_bonus` (gleiche Advancement-Formel, gleiche `pawn_passed_rank_bonuses`),
/// hier separat fuer `material_deficit_damping`. Bei Aenderung der Formel beide
/// Stellen anpassen.
fn side_passed_bonus_sum<B: EngineBoard>(board: &B, us: Color, p: &EvalParams) -> i32 {
    let our_bb = *board.color_combined(us);
    let our_pawns = *board.pieces(Piece::Pawn) & our_bb;
    let their_pawns = *board.pieces(Piece::Pawn) & *board.color_combined(!us);
    let mut sum = 0;
    for sq in our_pawns {
        if is_passed(sq, us, their_pawns) {
            let rank_idx = sq.get_rank().to_index();
            let advancement = match us {
                Color::White => rank_idx.saturating_sub(1),
                Color::Black => 6usize.saturating_sub(rank_idx),
            }
            .min(p.pawn_passed_rank_bonuses.len() - 1);
            sum += p.pawn_passed_rank_bonuses[advancement];
        }
    }
    sum
}

/// Akkumuliert den PST-Beitrag einer Seite in (mg, eg).
fn pst_score<B: EngineBoard>(board: &B, us: Color) -> (i32, i32) {
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

fn evaluate_side<B: EngineBoard>(board: &B, us: Color, p: &EvalParams, phase: i32) -> i32 {
    let mut score: i32 = 0;

    let our_bb = *board.color_combined(us);
    let their_pawns = *board.pieces(Piece::Pawn) & *board.color_combined(!us);
    let our_pawns = *board.pieces(Piece::Pawn) & our_bb;

    // Vorab für die dynamische Material-Bewertung (Schritt 2):
    //   - own_pawn_count: skaliert den Springer (Pawn-Kooperation)
    //   - total_pawn_count: skaliert den Läufer (Linien-Offenheit)
    // Beide Werte sind innerhalb von evaluate_side konstant — günstiger,
    // sie einmal pro Seite zu zählen statt in jedem Schleifendurchlauf neu.
    let own_pawn_count = our_pawns.popcnt() as i32;
    let total_pawn_count = own_pawn_count + their_pawns.popcnt() as i32;
    let home_rank = match us {
        Color::White => Rank::First,
        Color::Black => Rank::Eighth,
    };

    // Pro Figur: Materialwert + figurenspezifische Boni
    for sq in our_bb {
        let Some(piece) = board.piece_on(sq) else {
            continue;
        };
        score += piece_material(piece, p, phase, own_pawn_count, total_pawn_count);

        match piece {
            Piece::Pawn => {
                score += pawn_bonus(sq, us, our_pawns, their_pawns, p);
            }
            Piece::Knight => {
                let rank = sq.get_rank();
                if rank == Rank::First || rank == Rank::Eighth {
                    score += p.knight_backrank_penalty;
                }
                // Outpost-Bonus (Diagnose 05.06.2026): gedeckter, unvertreibbarer
                // Springer auf vorgeschobenem Feld. Code-Default 0 → inaktiv.
                if is_knight_outpost(sq, us, our_pawns, their_pawns) {
                    score += taper(p.outpost_knight_mg, p.outpost_knight_eg, phase);
                }
            }
            Piece::Bishop => {
                // Entwicklungs-Malus (960-Lookback 26.07.2026): Laeufer auf der
                // eigenen Grundreihe kostet im Mittelspiel Tempo-Aequivalent.
                // Nur MG-Pol getapert — im Endspiel ist die Grundreihe legitim.
                if sq.get_rank() == home_rank {
                    score += taper(p.bishop_backrank_penalty_mg, 0, phase);
                }
            }
            _ => {}
        }
    }

    // Laeuferpaar — Schritt 3 der dynamischen Figurenbewertung (16.05.2026):
    // statt konstantem `2 * bishop_pair_each` (=30) jetzt phasen-getapert,
    // Endspiel-Pol zusaetzlich nach Brett-Offenheit moduliert (mehr fehlende
    // Bauern → freiere Diagonalen → wertvolleres Paar).
    // Defaults im Code (30/30/0) reproduzieren das alte statische Verhalten;
    // wirksame Werte stehen in eval.toml `[material_dynamic]`.
    let our_bishops = *board.pieces(Piece::Bishop) & our_bb;
    if our_bishops.popcnt() >= 2 {
        let eg_value = p.bishop_pair_eg + (16 - total_pawn_count) * p.bp_open_scale;
        score += taper(p.bishop_pair_mg, eg_value, phase);
    }

    // Verbundene Tuerme — einmal pro Paar
    let our_rooks = *board.pieces(Piece::Rook) & our_bb;
    if rooks_connected(board, our_rooks) {
        score += p.connected_rooks_pair;
    }

    // Tuerme auf offenen / halb-offenen Linien
    score += rook_file_bonus(our_rooks, our_pawns, their_pawns, p);

    // Tarrasch-Regel: Turm-Freibauer-Koordination auf gleicher Linie
    score += rook_passed_pawn_bonus(us, our_rooks, our_pawns, their_pawns, p);

    // Turm auf 7. Reihe (besonders stark, wenn gegnerischer König auf 8.)
    score += rook_seventh_rank_bonus(board, us, our_rooks, p);

    // Bauernphalanx (reihenweise)
    score += phalanx_bonus(our_pawns, p);

    // King Safety (positiv = sicher, negativ = in Gefahr)
    score += king_safety_with_phase(board, us, p, phase);

    score
}

/// Gesamt-Beitrag der King-Safety-Logik aus Sicht von `us`.
/// Positiver Wert = sicher, negativer Wert = in Gefahr.
///
/// Setzt sich aus drei Termen zusammen:
///  - `shield`   (+): Bauernschild drei Linien breit vor dem König
///  - `danger`   (-): Angriffsgewichte gegnerischer Offiziere auf die 3×3-Zone
///  - `exposure` (-): Malus für König zu weit vom Heimrand, wenn der Gegner
///                    noch Schwergewicht-Material hat (siehe king_exposure_penalty)
#[cfg(test)]
fn king_safety<B: EngineBoard>(board: &B, us: Color, p: &EvalParams) -> i32 {
    king_safety_with_phase(board, us, p, game_phase(board))
}

fn king_safety_with_phase<B: EngineBoard>(board: &B, us: Color, p: &EvalParams, phase: i32) -> i32 {
    let king_sq = board.king_square(us);
    let zone = king_zone(king_sq);

    let shield = pawn_shield_score(board, us, king_sq, p);
    let danger = king_danger(board, us, zone, p);
    let exposure = king_exposure_penalty(board, us, p, phase);

    // Rochade-Anreiz (960-Lookback 26.07.2026): solange die Seite noch
    // Rochaderechte hat, wurde nicht rochiert → kleiner MG-Malus. Die
    // Rochade selbst loescht die Rechte und damit den Malus — sie "verdient"
    // den Betrag also als Tempo-Bonus. Verliert die Seite die Rechte durch
    // Koenigs-/Turmzuege, faellt der Malus zwar auch weg, aber dann greifen
    // Zentrums- (d/e) bzw. Flanken-Strafe (a/f) in `pawn_shield_score`.
    // Negativer Parameter, Default 0 = inaktiv.
    let castle_nudge = if board.has_castle_rights_for(us) {
        taper(p.ks_castle_rights_penalty_mg, 0, phase)
    } else {
        0
    };

    shield - danger - exposure + castle_nudge
}

/// König-Expositions-Strafe (eingeführt 2026-04-22, entschärft 2026-04-26).
///
/// Ergänzt `shield` und `danger` um einen "König ist zu weit vom Heimrand
/// weg, während der Gegner noch Schwergewicht hat"-Term. Motiviert durch
/// das mochi_bot-Spiel (EY25JUSH), in dem Martuni mit 16...Kg4 aus dem
/// Schach in die gegnerischen Figuren hineinlief — Kg6 wäre sicher gewesen.
/// Die bisherige Heuristik aus `shield` (deaktiviert sich abseits der
/// Heimreihe) und `danger` (wertet nur die 3×3-Nahzone) erkannte die
/// Gefahr nicht stark genug: Kg6 und Kg4 bekamen praktisch denselben Score.
///
/// Tuning vom 26.04.2026 nach 167 Lichess-Partien: weight 20 → 12,
/// rank_dist-Schwelle 2 → 3, plus Phase-Tapering. Auswertung hatte gezeigt,
/// dass der Term im Endspiel-Übergang den König "festfror" — Endgame-
/// Blunder pro Partie waren von 0.33 auf 0.49 gestiegen, mehrere Stellungen
/// mit 500–1000cp Eval-Pessimismus gegenüber Stockfish.
///
/// Formel:
/// ```text
///   rank_dist = |rank(König) - home_rank|    (0..7)
///   enemy_npm = Σ Figurenwerte des Gegners (Springer+Läufer+Turm+Dame)
///   phase_factor = min(phase, threshold) / threshold
///   Gate: rank_dist >= 3     (König auf Heim- oder ersten zwei vorgerückten Reihen ok)
///   Gate: enemy_npm >= 1500  (sonst aktiver König im Endspiel erwünscht)
///   exposure = (rank_dist - 2) * enemy_npm / 1000
///   penalty  = exposure * king_exposure_weight * phase_factor
/// ```
///
/// Beispiel Mochi 16...Kg4 (schwarz, phase ≈ 18, also voll):
///   rank_dist=4, weiß-NPM=1600cp
///   exposure = 2 * 1600 / 1000 = 3 (Integer-Div)
///   penalty  = 3 * 12 * 1 = 36cp Abzug
/// Vergleich Kg6: rank_dist=2 → unter Schwelle → 0cp.
/// Differenz Kg4↔Kg6 = 36cp (vorher 60cp, aber dort hatte auch Kg6 schon
/// 20cp Malus — jetzt ist die Trennung klarer: Kg6 wird nicht mehr bestraft).
///
/// Phase-Tapering: bei phase=8 (R+R+N+N-Endspiel) zählt nur halb (×0.5),
/// bei phase=0 (KP-Endspiel) gar nicht. So überlässt der Term die
/// Endspielphase wieder `king_activity_endgame`.
///
/// Rückgabewert ist positiv (als "abzuziehender Malus" im Aufrufer gedacht).
fn king_exposure_penalty<B: EngineBoard>(board: &B, us: Color, p: &EvalParams, phase: i32) -> i32 {
    // Abstand des Königs zu seiner Grundreihe
    let king_sq = board.king_square(us);
    let rank = king_sq.get_rank().to_index() as i32;
    let home_rank = match us {
        Color::White => 0,
        Color::Black => 7,
    };
    let rank_dist = (rank - home_rank).abs();

    // rank_dist < 3: König auf Heim- oder ersten beiden vorgerückten Reihen.
    // Das deckt die typische Rochadeposition + Bauernverlust + leichtes
    // Vorrücken in Bauernendspielen ab. Erst ab rank_dist=3 (Reihe 4/5) ist
    // er klar im "Niemandsland".
    if rank_dist < 3 {
        return 0;
    }

    // Gegnerisches Nicht-Bauern-Material in cp
    let enemy = !us;
    let enemy_bb = *board.color_combined(enemy);
    let mut enemy_npm = 0;
    enemy_npm += (*board.pieces(Piece::Knight) & enemy_bb).popcnt() as i32 * p.knight;
    enemy_npm += (*board.pieces(Piece::Bishop) & enemy_bb).popcnt() as i32 * p.bishop;
    enemy_npm += (*board.pieces(Piece::Rook) & enemy_bb).popcnt() as i32 * p.rook;
    enemy_npm += (*board.pieces(Piece::Queen) & enemy_bb).popcnt() as i32 * p.queen;

    // Unter dieser Schwelle hat der Gegner nicht mehr genug Feuerkraft,
    // um einen exponierten König effektiv zu bestrafen. Die Endspiel-
    // Termini (king_activity_endgame) übernehmen dann die Bewertung des
    // aktiven Königs — und die wollen wir nicht übersteuern.
    if enemy_npm < 1500 {
        return 0;
    }

    // Phase-Faktor: 1.0 ab Mittelspiel-Schwelle, linear gegen 0 im Endspiel.
    // Komplementär zu `king_activity_endgame`, das genau dann Vollgas gibt,
    // wenn der Exposure-Term hier verschwindet.
    let threshold = p.king_activity_phase_threshold;
    let phase_clamped = phase.min(threshold);

    // (rank_dist - 2): rank_dist=3 → Faktor 1, rank_dist=7 → Faktor 5.
    // enemy_npm / 1000 als grobe Schwergewichts-Skala.
    let exposure = (rank_dist - 2) * enemy_npm / 1000;
    exposure * p.king_exposure_weight * phase_clamped / threshold
}

/// 3x3-Koenigszone: der Koenig selbst + alle acht Nachbarfelder.
/// Nutzt die interne Lookup-Tabelle der chess-Crate.
fn king_zone(sq: Square) -> BitBoard {
    get_king_moves(sq) | BitBoard::from_square(sq)
}

/// Summiert Angriffsgewichte gegnerischer Offiziere auf die King-Zone,
/// indiziert SafetyTable und liefert die Strafe in cp.
fn king_danger<B: EngineBoard>(board: &B, us: Color, zone: BitBoard, p: &EvalParams) -> i32 {
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
///
/// Shield-Bonus nur fuer Rochade-Endfelder plus den ueblichen
/// Post-Rochade-Sicherheitszug:
///   - g (file 6, kurze Rochade) oder h (file 7, Kh1 nach OO)
///   - c (file 2, lange Rochade) oder b (file 1, Kb1 nach OOO)
/// Auf f- oder a-Linie steht der Koenig typischerweise nur nach
/// erzwungenem Schach oder ohne Rochade — kein Bonus und keine Strafe
/// (return 0).
/// Historischer Hintergrund: bis 24.05.2026 wurde jeder Koenig auf
/// Grundreihe abseits d/e wie ein rochierter Koenig bewertet — sxphia-
/// Patzer 9...Nxc4 (Bz0YIF49) trat auf, weil Martuni dem Kf8-nach-Bb5+
/// einen +30cp Shield-Bonus gab.
fn pawn_shield_score<B: EngineBoard>(board: &B, us: Color, king_sq: Square, p: &EvalParams) -> i32 {
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

    // Shield-Bonus nur fuer Rochade-Endfelder plus den ueblichen
    // Post-Rochade-Sicherheitszug:
    //   file 2 (c) = lang rochiert / file 1 (b) = Kb1 nach OOO
    //   file 6 (g) = kurz rochiert / file 7 (h) = Kh1 nach OO
    // Auf der f-Linie oder a-Linie steht der Koenig typischerweise nur
    // nach erzwungenem Schach (verlorene Rochaderechte) — kein Bonus,
    // seit 26.07.2026 stattdessen ein eigener Malus (960-Lookback:
    // unrochierte Koenige waren Blunder-Motiv #2; vorher 0).
    let (file_lo, file_hi) = match king_file {
        1 | 2 => (0, 2),
        6 | 7 => (5, 7),
        _ => return p.ks_uncastled_flank_penalty,
    };
    let (r1, r2) = match us {
        Color::White => (1, 2),
        Color::Black => (6, 5),
    };

    let our_pawns = *board.pieces(Piece::Pawn) & *board.color_combined(us);
    let mut score = 0;

    for f in file_lo..=file_hi {
        let sq_r1 = Square::make_square(Rank::from_index(r1), File::from_index(f as usize));
        let sq_r2 = Square::make_square(Rank::from_index(r2), File::from_index(f as usize));
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

/// Materialwert einer Figur in Centipawns.
///
/// Springer und Läufer nutzen MG/EG-Tapering (dynamische Figurenbewertung,
/// Schritt 1) plus optionales Pawn-Adjustment (Schritt 2). Die übrigen
/// Figuren bleiben statisch — der Bauer ist definitionsgemäß die
/// Centipawn-Skala selbst (100), Turm und Dame sind genug standardisierte
/// Werte, dass eine Phase-Abhängigkeit keinen belastbaren Mehrwert
/// verspricht.
///
/// Pawn-Adjustment (Step 2, Kaufman 1999):
///   * **Springer** profitiert von eigenen Bauern — sie liefern Hilfs-
///     struktur und Outpost-Stützen. Bei 8 eigenen Bauern (Startposition)
///     ist der Adjustment 0; jeder verlorene eigene Bauer kostet
///     `knight_pawn_scale` cp.
///   * **Läufer** profitiert von offener Gesamt-Stellung (Diagonalen-
///     Sichtweiten). Bei vollem Brett (16 Bauern) Adjustment 0; jeder
///     fehlende Bauer (egal welche Farbe) gibt `bishop_pawn_scale` cp.
/// Beide Skalen sind 0-Default → ohne `eval.toml`-Override identisch zu
/// Step 1.
///
/// Wichtige Abgrenzung: dieser Wert wird *nur* im Per-Figur-Materialscore
/// von `evaluate_side` verwendet. Andere Stellen, die einen Materialwert
/// brauchen — `king_exposure_penalty` (NPM-Schwelle 1500cp) und
/// `endgame::strong_material` (Endspiel-Klassifizierung) —, greifen weiter
/// auf `p.knight` / `p.bishop` direkt zu. Das ist Absicht: jene Terme
/// definieren ihre eigenen Schwellen relativ zu einem festen Anker, und
/// eine wandernde Figurenbewertung würde dort die Anker mitbewegen.
fn piece_material(
    piece: Piece,
    p: &EvalParams,
    phase: i32,
    own_pawn_count: i32,
    total_pawn_count: i32,
) -> i32 {
    match piece {
        Piece::Pawn => p.pawn,
        Piece::Knight => {
            let base = taper(p.knight_mg, p.knight_eg, phase);
            let adj = (own_pawn_count - 8) * p.knight_pawn_scale;
            base + adj
        }
        Piece::Bishop => {
            let base = taper(p.bishop_mg, p.bishop_eg, phase);
            let adj = (16 - total_pawn_count) * p.bishop_pawn_scale;
            base + adj
        }
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

    if is_isolated(our_pawns, sq.get_file()) {
        b += p.pawn_isolated_penalty;
    }

    if is_passed(sq, us, their_pawns) {
        // Bonus skaliert mit dem Vormarsch-Rang des Freibauers.
        // advancement = 0 (Ausgangsreihe) bis 5 (ein Schritt vor Umwandlung).
        let rank_idx = sq.get_rank().to_index();
        let advancement = match us {
            Color::White => rank_idx.saturating_sub(1),
            Color::Black => 6usize.saturating_sub(rank_idx),
        }
        .min(p.pawn_passed_rank_bonuses.len() - 1);
        b += p.pawn_passed_rank_bonuses[advancement];
    }

    b
}

#[inline]
fn is_isolated(our_pawns: BitBoard, file: File) -> bool {
    (our_pawns & get_adjacent_files(file)) == BitBoard::new(0)
}

// pub(crate): wird auch von der Check-Extension in search.rs genutzt
// (frueher dort als identische Kopie `is_passed_simple` dupliziert).
#[inline]
pub(crate) fn is_passed(sq: Square, us: Color, their_pawns: BitBoard) -> bool {
    let file_idx = sq.get_file().to_index() as i32;
    let rank_idx = sq.get_rank().to_index() as i32;

    let mut file_mask: u64 = 0;
    for df in [-1i32, 0, 1] {
        let f = file_idx + df;
        if (0..8).contains(&f) {
            file_mask |= 0x0101_0101_0101_0101u64 << (f as u32);
        }
    }

    let ahead_mask = match us {
        Color::White => {
            if rank_idx >= 7 {
                0
            } else {
                !0u64 << (8 * (rank_idx + 1) as u32)
            }
        }
        Color::Black => {
            if rank_idx <= 0 {
                0
            } else {
                (1u64 << (8 * rank_idx as u32)) - 1
            }
        }
    };

    // Vorher wurde jedes der maximal 18 relevanten Felder einzeln gebaut und
    // geprueft. Die Bitmaske beschreibt exakt dieselben Felder: Nachbarlinien
    // plus eigene Linie, aber nur vor dem Bauern.
    (their_pawns.0 & file_mask & ahead_mask) == 0
}

/// Prueft, ob der Springer auf `sq` auf einem (gewerteten) Outpost steht.
///
/// Drei Bedingungen — alle muessen erfuellt sein:
///   1. **Vorgeschoben:** 4.-6. Reihe aus eigener Sicht. Ein Outpost auf der
///      eigenen Haelfte bringt keinen Raumgewinn; erst ab der 4. Reihe wird der
///      Springer zum dauerhaften Stachel in der gegnerischen Stellung.
///   2. **Gedeckt:** ein eigener Bauer steht diagonal dahinter (Nachbarlinie,
///      eine Reihe Richtung eigene Grundreihe). Nur ein gedeckter Springer ist
///      stabil — ungedeckte "Outposts" verschenken sich oft taktisch.
///   3. **Loch:** kein gegnerischer Bauer auf den Nachbarlinien VOR dem Springer.
///      Solche Bauern koennten vorruecken und den Springer vertreiben; fehlen
///      sie, ist das Feld ein permanentes Loch in der gegnerischen Struktur.
///
/// Dieses Wissen erfasst weder PST (statisch pro Feld) noch Safe-Mobility
/// (zaehlt nur Zielfelder) — es war eine der Eval-Luecken hinter den
/// "motivlosen" Mittelspiel-Drops (Diagnose 05.06.2026, docs/roadmap.md).
fn is_knight_outpost(sq: Square, us: Color, our_pawns: BitBoard, their_pawns: BitBoard) -> bool {
    let rank_idx = sq.get_rank().to_index() as i32;

    // (1) Relative Reihe: 0 = eigene Grundreihe, 7 = Umwandlungsreihe.
    let rel_rank = match us {
        Color::White => rank_idx,
        Color::Black => 7 - rank_idx,
    };
    if !(3..=5).contains(&rel_rank) {
        return false; // nur 4., 5. oder 6. Reihe
    }

    // Nachbarlinien (ohne die eigene Linie) — Bauern auf derselben Linie koennen
    // einen Springer weder decken noch (diagonal) angreifen.
    let adj = get_adjacent_files(sq.get_file());

    // (2) Bauern-Deckung: eigener Bauer eine Reihe Richtung Heimat, Nachbarlinie.
    let behind_rank = match us {
        Color::White => rank_idx - 1,
        Color::Black => rank_idx + 1,
    };
    if !(0..8).contains(&behind_rank) {
        return false;
    }
    let behind_mask = BitBoard::new(0xffu64 << (8 * behind_rank as u32));
    if (our_pawns & adj & behind_mask) == BitBoard::new(0) {
        return false; // ungedeckt → kein gewerteter Outpost
    }

    // (3) "Loch": kein Gegnerbauer auf den Nachbarlinien vor dem Springer
    //     (gleiche ahead-Maske wie `is_passed`, hier nur auf Nachbarlinien).
    let ahead_mask: u64 = match us {
        Color::White => {
            if rank_idx >= 7 {
                0
            } else {
                !0u64 << (8 * (rank_idx + 1) as u32)
            }
        }
        Color::Black => {
            if rank_idx <= 0 {
                0
            } else {
                (1u64 << (8 * rank_idx as u32)) - 1
            }
        }
    };
    (their_pawns & adj & BitBoard::new(ahead_mask)) == BitBoard::new(0)
}

fn phalanx_bonus(our_pawns: BitBoard, p: &EvalParams) -> i32 {
    let mut total: i32 = 0;
    for rank_idx in 0..8 {
        let mut rank_bits = (our_pawns.0 >> (rank_idx * 8)) & 0xff;
        let mut run: usize = 0;
        for _ in 0..8 {
            if rank_bits & 1 != 0 {
                run += 1;
            } else {
                total += score_run(run, p);
                run = 0;
            }
            rank_bits >>= 1;
        }
        total += score_run(run, p);
    }
    total
}

#[inline]
fn score_run(len: usize, p: &EvalParams) -> i32 {
    if len >= 3 {
        p.pawn_phalanx_triple
    } else if len == 2 {
        p.pawn_phalanx_double
    } else {
        0
    }
}

fn rooks_connected<B: EngineBoard>(board: &B, our_rooks: BitBoard) -> bool {
    if our_rooks.popcnt() < 2 {
        return false;
    }
    let occ = *board.combined();
    for rook_sq in our_rooks {
        let other_rooks = our_rooks ^ BitBoard::from_square(rook_sq);
        // get_rook_moves liefert auf belegten Linien die Felder bis zum ersten
        // Blocker inklusive. Schneidet diese Maske einen zweiten eigenen Turm,
        // ist exakt die alte "have_sight"-Bedingung erfuellt, nur ohne Vec und
        // ohne manuelle Square-fuer-Square-Schleife.
        if (get_rook_moves(rook_sq, occ) & other_rooks) != BitBoard::new(0) {
            return true;
        }
    }
    false
}

/// Bonus für Türme auf offenen und halb-offenen Linien.
/// Offene Linie: keine eigenen UND keine gegnerischen Bauern.
/// Halb-offene Linie: keine eigenen, aber gegnerische Bauern vorhanden.
fn rook_file_bonus(
    our_rooks: BitBoard,
    our_pawns: BitBoard,
    their_pawns: BitBoard,
    p: &EvalParams,
) -> i32 {
    let mut score = 0;
    for sq in our_rooks {
        let mask = get_file(sq.get_file());
        let own_on_file = (our_pawns & mask) != BitBoard::new(0);
        let their_on_file = (their_pawns & mask) != BitBoard::new(0);
        if !own_on_file && !their_on_file {
            score += p.rook_open_file_bonus;
        } else if !own_on_file {
            score += p.rook_semiopen_file_bonus;
        }
    }
    score
}

/// Tarrasch-Regel: Turm gehört hinter den Freibauer.
///
/// Für jedes Turm-Freibauer-Paar auf derselben Linie:
///  - eigener Turm hinter eigenem Freibauer → +bonus (schiebt den Bauer)
///  - eigener Turm VOR eigenem Freibauer → -penalty (blockt sich selbst)
///  - eigener Turm hinter gegnerischem Freibauer → +bonus (Blockade von hinten)
///
/// "Hinter" ist aus Sicht des Bauern zu lesen: Weißer Bauer läuft nach Norden,
/// also ist der rückwärtige Sektor (kleinere Reihe) "hinter" ihm. Schwarzer
/// Bauer entsprechend spiegelverkehrt.
fn rook_passed_pawn_bonus(
    us: Color,
    our_rooks: BitBoard,
    our_pawns: BitBoard,
    their_pawns: BitBoard,
    p: &EvalParams,
) -> i32 {
    let mut score = 0;
    for rook_sq in our_rooks {
        let file_mask = get_file(rook_sq.get_file());
        let rook_rank = rook_sq.get_rank().to_index() as i32;

        // Eigene Freibauern auf derselben Linie (maximal einer — ein zweiter
        // wäre kein Freibauer, weil der erste auf derselben Linie stünde).
        for pawn_sq in our_pawns & file_mask {
            if !is_passed(pawn_sq, us, their_pawns) {
                continue;
            }
            let pawn_rank = pawn_sq.get_rank().to_index() as i32;
            if rook_is_behind_pawn(rook_rank, pawn_rank, us) {
                score += p.rook_behind_own_passed_bonus;
            } else if rook_rank != pawn_rank {
                // Turm vor eigenem Freibauer → Malus (blockt Vormarsch).
                score += p.rook_blocks_own_passed_penalty;
            }
        }

        // Gegnerische Freibauern auf derselben Linie: Turm hinter ihnen
        // (aus Bauer-Sicht) ist der klassische "Blockade-von-hinten"-Bonus.
        for pawn_sq in their_pawns & file_mask {
            if !is_passed(pawn_sq, !us, our_pawns) {
                continue;
            }
            let pawn_rank = pawn_sq.get_rank().to_index() as i32;
            if rook_is_behind_pawn(rook_rank, pawn_rank, !us) {
                score += p.rook_behind_enemy_passed_bonus;
            }
        }
    }
    score
}

/// Prüft, ob ein Turm hinter einem Bauern (dessen Farbe `pawn_color`) steht.
/// Weißer Bauer läuft Richtung Rang 7 → hinter ihm = Rang < pawn_rank.
/// Schwarzer Bauer läuft Richtung Rang 0 → hinter ihm = Rang > pawn_rank.
#[inline]
fn rook_is_behind_pawn(rook_rank: i32, pawn_rank: i32, pawn_color: Color) -> bool {
    match pawn_color {
        Color::White => rook_rank < pawn_rank,
        Color::Black => rook_rank > pawn_rank,
    }
}

/// Turm auf 7. Reihe aus eigener Sicht. Extra-Bonus, wenn der gegnerische
/// König auf der 8. (Grund-)Reihe eingesperrt steht — dann schneidet der
/// Turm eine Fluchtlinie ab ("pig on the seventh").
fn rook_seventh_rank_bonus<B: EngineBoard>(board: &B, us: Color, our_rooks: BitBoard, p: &EvalParams) -> i32 {
    let (seventh_rank, eighth_rank) = match us {
        Color::White => (6, 7),
        Color::Black => (1, 0),
    };
    let enemy_king_rank = board.king_square(!us).get_rank().to_index() as i32;

    let mut score = 0;
    for rook_sq in our_rooks {
        if rook_sq.get_rank().to_index() as i32 == seventh_rank {
            score += p.rook_seventh_bonus;
            if enemy_king_rank == eighth_rank {
                score += p.rook_seventh_vs_king_eighth_bonus;
            }
        }
    }
    score
}

/// Endspiel-Malus: Turm direkt hinter eigenem Bauern eingemauert.
/// Sehr konservativ definiert (kleinste Wirkung, Tobias' Wunsch):
///  - Turm auf Heimreihe oder zweiter Reihe (rank 0/1 weiß, 7/6 schwarz)
///  - Eigener Bauer auf der Linie direkt vor dem Turm (eine Reihe weiter)
///
/// Die Funktion gibt einen rohen Endspielwert zurück (nicht getapert);
/// das Tapern erledigt der Aufrufer in `evaluate()`. Im vollen Mittelspiel
/// trägt der Term damit nichts bei — der Bauer dient dort der King-Safety
/// oder der Eröffnungsstruktur. Im Endspiel zieht sie den Turm aktiv heraus.
fn rook_trapped_endgame_malus<B: EngineBoard>(board: &B, us: Color, p: &EvalParams) -> i32 {
    let our_bb = *board.color_combined(us);
    let our_rooks = *board.pieces(Piece::Rook) & our_bb;
    let our_pawns = *board.pieces(Piece::Pawn) & our_bb;

    let (home_rank, second_rank, dir) = match us {
        Color::White => (0i32, 1i32, 1i32),
        Color::Black => (7i32, 6i32, -1i32),
    };

    let mut malus = 0;
    for rook_sq in our_rooks {
        let rank = rook_sq.get_rank().to_index() as i32;
        if rank != home_rank && rank != second_rank {
            continue;
        }
        let blocker_rank = rank + dir;
        if !(0..=7).contains(&blocker_rank) {
            continue;
        }
        let blocker_sq =
            Square::make_square(Rank::from_index(blocker_rank as usize), rook_sq.get_file());
        if (our_pawns & BitBoard::from_square(blocker_sq)) != BitBoard::new(0) {
            malus += p.rook_trapped_endgame_penalty;
        }
    }
    malus
}

/// König-Freibauer-Synergie (Endspielterm).
///
/// Für jeden eigenen Freibauer: je näher der eigene König (Chebyshev), desto
/// mehr Bonus — der König soll den Bauer begleiten. Für jeden gegnerischen
/// Freibauer: Bonus, wenn der eigene König als Blockadefigur in der Nähe steht.
///
/// Das ganze wirkt nur im Endspiel (Phase < `king_activity_phase_threshold`)
/// und skaliert linear mit `(threshold - phase) / threshold`, analog zu
/// `king_activity_endgame`. So verhindern wir, dass der König schon im
/// Mittelspiel vor seinen Bauern herläuft und die Rochadesicherheit aufgibt.
fn king_passed_pawn_synergy<B: EngineBoard>(board: &B, phase: i32, p: &EvalParams) -> i32 {
    if phase >= p.king_activity_phase_threshold {
        return 0;
    }
    let w = side_king_passed_synergy(board, Color::White, p);
    let b = side_king_passed_synergy(board, Color::Black, p);
    let eg_weight = p.king_activity_phase_threshold - phase;
    (w - b) * eg_weight / p.king_activity_phase_threshold
}

fn side_king_passed_synergy<B: EngineBoard>(board: &B, us: Color, p: &EvalParams) -> i32 {
    let king_sq = board.king_square(us);
    let our_pawns = *board.pieces(Piece::Pawn) & *board.color_combined(us);
    let their_pawns = *board.pieces(Piece::Pawn) & *board.color_combined(!us);

    let mut score = 0;
    for pawn_sq in our_pawns {
        if !is_passed(pawn_sq, us, their_pawns) {
            continue;
        }
        let d = crate::endgame::chebyshev(king_sq, pawn_sq);
        score += (7 - d) * p.king_near_own_passed_bonus;
    }
    for pawn_sq in their_pawns {
        // Aus gegnerischer Sicht: der Gegner ist für seinen Bauer "us".
        if !is_passed(pawn_sq, !us, our_pawns) {
            continue;
        }
        let d = crate::endgame::chebyshev(king_sq, pawn_sq);
        score += (7 - d) * p.king_near_enemy_passed_bonus;
    }
    score
}

// =========================================================================
// Pawn-Endgame-Guard (23.05.2026)
//
// Ergaenzt die bestehenden Endspielterme (`king_activity_endgame`,
// `king_passed_pawn_synergy`, `endgame::endgame_score`) um drei klassische
// Pawn-Endgame-Konzepte:
//   1. Opposition (direkt + diagonal): Bonus, wenn der eigene Koenig die
//      Opposition gegen den gegnerischen haelt.
//   2. Key Squares: Bonus pro eigenem Freibauer, wenn der eigene Koenig
//      auf einem der drei Schluesselfelder dieses Bauern steht.
//   3. Rook-Pawn-Edge: Daempfung des Passbauer-Optimismus bei a-/h-Bauer
//      + gegnerischem Koenig in der Promo-Ecke.
//
// Konzept: docs/pawn-endgame-guard.md. Aktivierungsbedingung: NPM beider
// Seiten ≤ `npm_endgame_gate` (hartes Gate), Phase unterhalb
// `king_activity_phase_threshold`, Phase-Tapering analog
// `king_passed_pawn_synergy`.
// =========================================================================

/// Top-Level-Aufruf in `evaluate()`. Liefert die Diff (Weiss - Schwarz)
/// schon getapert in den Endspielpol.
fn pawn_endgame_guard<B: EngineBoard>(board: &B, phase: i32, p: &EvalParams) -> i32 {
    if !is_simple_pawn_endgame(board, p) {
        return 0;
    }
    if phase >= p.king_activity_phase_threshold {
        return 0;
    }
    let w = side_pawn_endgame_guard(board, Color::White, p);
    let b = side_pawn_endgame_guard(board, Color::Black, p);
    let eg_weight = p.king_activity_phase_threshold - phase;
    (w - b) * eg_weight / p.king_activity_phase_threshold
}

/// Hardes NPM-Gate. Misst nicht-Bauern-Material beider Seiten mit den
/// statischen Anker-Werten (p.knight, p.bishop, p.rook, p.queen). Beide
/// Seiten muessen unter dem Gate liegen, damit der Guard greift — sonst
/// ist es kein einfaches Pawn-Endgame mehr.
fn is_simple_pawn_endgame<B: EngineBoard>(board: &B, p: &EvalParams) -> bool {
    p.npm_endgame_gate > 0
        && side_npm(board, Color::White, p) <= p.npm_endgame_gate
        && side_npm(board, Color::Black, p) <= p.npm_endgame_gate
}

fn side_npm<B: EngineBoard>(board: &B, us: Color, p: &EvalParams) -> i32 {
    let bb = *board.color_combined(us);
    let n = (*board.pieces(Piece::Knight) & bb).popcnt() as i32;
    let bp = (*board.pieces(Piece::Bishop) & bb).popcnt() as i32;
    let r = (*board.pieces(Piece::Rook) & bb).popcnt() as i32;
    let q = (*board.pieces(Piece::Queen) & bb).popcnt() as i32;
    n * p.knight + bp * p.bishop + r * p.rook + q * p.queen
}

/// Aufaddierter Guard-Beitrag einer Seite (ohne Phase-Skalierung).
fn side_pawn_endgame_guard<B: EngineBoard>(board: &B, us: Color, p: &EvalParams) -> i32 {
    let mut score = 0;
    score += opposition_bonus(board, us, p);
    score += key_square_bonus(board, us, p);
    score += rook_pawn_correction(board, us, p);
    score
}

/// Opposition-Erkennung (direkt + diagonal) im ersten Wurf:
///
/// Gilt die Opposition, wenn
///   - die beiden Koenige auf gleicher Datei, Reihe ODER Diagonale stehen,
///   - die Anzahl freier Felder zwischen ihnen UNGERADE ist, und
///   - der GEGNER am Zug ist.
///
/// Hinweis zur Konvention: in der klassischen Schach-Theorie heisst "die
/// Opposition haben" = "der andere ist am Zug". Der Bonus geht also an die
/// Seite, die NICHT am Zug ist (sofern die Geometrie passt).
fn opposition_bonus<B: EngineBoard>(board: &B, us: Color, p: &EvalParams) -> i32 {
    if p.opposition_bonus == 0 {
        return 0;
    }
    // Wenn wir am Zug sind, haben wir die Opposition nicht.
    if board.side_to_move() == us {
        return 0;
    }
    let our_k = board.king_square(us);
    let their_k = board.king_square(!us);
    if !has_opposition_geometry(board, our_k, their_k) {
        return 0;
    }
    p.opposition_bonus
}

/// Geometrische Pruefung: gleiche Linie/Reihe/Diagonale, ungerade Anzahl
/// leerer Felder dazwischen. "Leer" heisst hier "keine Figur" — Bauern auf
/// der Linie/Reihe zwischen den Koenigen brechen die Opposition.
fn has_opposition_geometry<B: EngineBoard>(board: &B, a: Square, b: Square) -> bool {
    let af = a.get_file().to_index() as i32;
    let ar = a.get_rank().to_index() as i32;
    let bf = b.get_file().to_index() as i32;
    let br = b.get_rank().to_index() as i32;

    let df = bf - af;
    let dr = br - ar;

    // Schrittweite (-1, 0, +1) entlang File und Rank.
    let (step_f, step_r) = if df == 0 && dr != 0 {
        (0, dr.signum())
    } else if dr == 0 && df != 0 {
        (df.signum(), 0)
    } else if df.abs() == dr.abs() && df != 0 {
        (df.signum(), dr.signum())
    } else {
        return false; // weder Linie, Reihe noch Diagonale
    };

    // Anzahl Felder dazwischen = max(|df|,|dr|) - 1.
    let between = df.abs().max(dr.abs()) - 1;
    // Opposition braucht ungerade Anzahl freier Felder dazwischen.
    if between % 2 == 0 {
        return false;
    }

    // Pruefen, ob die Felder dazwischen wirklich leer sind.
    let mut f = af + step_f;
    let mut r = ar + step_r;
    for _ in 0..between {
        let sq = Square::make_square(rank_from_index(r), file_from_index(f));
        if board.piece_on(sq).is_some() {
            return false;
        }
        f += step_f;
        r += step_r;
    }
    true
}

#[inline]
fn file_from_index(i: i32) -> File {
    match i {
        0 => File::A,
        1 => File::B,
        2 => File::C,
        3 => File::D,
        4 => File::E,
        5 => File::F,
        6 => File::G,
        _ => File::H,
    }
}

#[inline]
fn rank_from_index(i: i32) -> Rank {
    match i {
        0 => Rank::First,
        1 => Rank::Second,
        2 => Rank::Third,
        3 => Rank::Fourth,
        4 => Rank::Fifth,
        5 => Rank::Sixth,
        6 => Rank::Seventh,
        _ => Rank::Eighth,
    }
}

/// Pro eigenem Freibauer: wenn der eigene Koenig auf einem der drei
/// Schluesselfelder steht, kassiere den rang-abhaengigen Bonus.
///
/// Schluesselfelder pro Bauer (aus Sicht des Bauern-Vorrueck-Rangs r,
/// 1=Heimreihe, 8=Promo-Reihe):
///   r ≤ 4  → drei Felder zwei Reihen vor dem Bauer (file-1, file, file+1)
///   r == 5 → wir nehmen die drei Felder DIREKT vor dem Bauer (kanonisches
///            Lehrbuch-Triplet). Das alternative Triplet zwei Reihen vor
///            dem Bauer faellt mit dem Bauer auf Rang 7 fast zusammen und
///            wird ueber `pawn_passed_rank_bonuses[5]` (=170 cp) bereits
///            erfasst.
///   r ≥ 6  → drei Felder direkt vor dem Bauer.
///
/// Bei Doppelbauern auf derselben Linie wird nur der weiter vorgerueckte
/// Bauer ausgezaehlt — verhindert Doppelzaehlung.
///
/// Rook-Pawns (a-/h-File) bekommen KEINEN Key-Square-Bonus — die Promo-
/// Ecke ist immer vom Verteidiger erreichbar; Sub-Konzept 3 uebernimmt.
fn key_square_bonus<B: EngineBoard>(board: &B, us: Color, p: &EvalParams) -> i32 {
    if p.key_square_bonus_by_rank.iter().all(|&x| x == 0) {
        return 0;
    }
    let our_bb = *board.color_combined(us);
    let our_pawns = *board.pieces(Piece::Pawn) & our_bb;
    let their_pawns = *board.pieces(Piece::Pawn) & *board.color_combined(!us);
    let king_sq = board.king_square(us);
    let king_f = king_sq.get_file().to_index() as i32;
    let king_r = king_sq.get_rank().to_index() as i32;

    let mut score = 0;
    let mut visited_files: u8 = 0;
    // Verarbeite Bauern in absteigender "Vorrueck-Reihenfolge", damit der
    // weiter vorgerueckte Bauer pro Linie zuerst kommt; spaetere Bauern auf
    // derselben Linie werden uebersprungen.
    let mut pawns_sorted: Vec<Square> = our_pawns.into_iter().collect();
    pawns_sorted.sort_by_key(|sq| {
        // Weisser Bauer: hoeherer Rank = weiter vorgerueckt. Schwarz spiegelverkehrt.
        let r = sq.get_rank().to_index() as i32;
        match us {
            Color::White => -r,
            Color::Black => r,
        }
    });

    for sq in pawns_sorted {
        let file = sq.get_file();
        let file_i = file.to_index() as i32;
        let file_mask = 1u8 << file_i;
        if visited_files & file_mask != 0 {
            continue; // schon der weiter vorgerueckte Bauer abgehakt
        }
        visited_files |= file_mask;

        // Rook-Pawn → kein Key-Square-Term hier (siehe Sub-Konzept 3).
        if file == File::A || file == File::H {
            continue;
        }
        if !is_passed(sq, us, their_pawns) {
            continue;
        }

        let rank_i = sq.get_rank().to_index() as i32; // 0..7
                                                      // "Bauern-Rang aus eigener Sicht": Weiss = rank_i+1, Schwarz = 8-rank_i.
        let rel_rank: i32 = match us {
            Color::White => rank_i + 1,
            Color::Black => 8 - rank_i,
        };
        if rel_rank < 2 || rel_rank > 7 {
            continue; // unmoeglich (Index 0 oder 7 in der Bonus-Tabelle)
        }
        let bonus = p.key_square_bonus_by_rank[(rel_rank - 1) as usize];
        if bonus == 0 {
            continue;
        }

        // Schluesselfelder-Triplet bestimmen, in Vorruecker-Richtung
        // gemessen (us=White → +rank, us=Black → -rank).
        let dir: i32 = match us {
            Color::White => 1,
            Color::Black => -1,
        };
        let key_rank_i: i32 = if rel_rank <= 4 {
            rank_i + 2 * dir
        } else if rel_rank == 5 {
            rank_i + dir
        } else {
            // rel_rank >= 6
            rank_i + dir
        };
        if !(0..=7).contains(&key_rank_i) {
            continue;
        }

        // Triplet: file-1, file, file+1 (Brett-File-Beschnitt).
        let key_files: [i32; 3] = [file_i - 1, file_i, file_i + 1];
        for kf in key_files {
            if !(0..=7).contains(&kf) {
                continue;
            }
            if king_f == kf && king_r == key_rank_i {
                score += bonus;
                break;
            }
        }
    }
    score
}

/// Wenn unser einziger Frei-Bauer ein Rook-Pawn (a- oder h-Bauer) ist
/// UND der gegnerische Koenig die Promo-Ecke schon kontrolliert (Chebyshev
/// ≤ 1) → Daempfung in Form einer fixen Strafe. Der Term ist als negative
/// Konstante gedacht; er reduziert den ueberschaetzten Optimismus des
/// Passbauer-Bonus, ohne den Materialwert des Bauern selbst anzutasten.
fn rook_pawn_correction<B: EngineBoard>(board: &B, us: Color, p: &EvalParams) -> i32 {
    if p.rook_pawn_drawish_penalty == 0 {
        return 0;
    }
    let our_bb = *board.color_combined(us);
    let our_pawns = *board.pieces(Piece::Pawn) & our_bb;
    let their_pawns = *board.pieces(Piece::Pawn) & *board.color_combined(!us);

    // Nur, wenn wir GENAU einen Frei-Bauer haben und der auf a/h liegt.
    let mut rook_pawn_sq: Option<Square> = None;
    let mut other_passed = 0;
    for sq in our_pawns {
        if !is_passed(sq, us, their_pawns) {
            continue;
        }
        let f = sq.get_file();
        if f == File::A || f == File::H {
            if rook_pawn_sq.is_some() {
                // Mehrere Rook-Pawn-Freibauern → komplexer, kein Edge-Case
                return 0;
            }
            rook_pawn_sq = Some(sq);
        } else {
            other_passed += 1;
        }
    }
    let pawn_sq = match rook_pawn_sq {
        Some(sq) => sq,
        None => return 0,
    };
    if other_passed > 0 {
        return 0;
    }

    // Promo-Ecke aus eigener Sicht: a8/h8 fuer Weiss, a1/h1 fuer Schwarz.
    let promo_rank = match us {
        Color::White => Rank::Eighth,
        Color::Black => Rank::First,
    };
    let promo_corner = Square::make_square(promo_rank, pawn_sq.get_file());
    let their_k = board.king_square(!us);
    if crate::endgame::chebyshev(their_k, promo_corner) <= 1 {
        return p.rook_pawn_drawish_penalty;
    }
    0
}

/// Bauernangriffe einer Seite als BitBoard. Weiße Bauern schlagen NE/NW
/// (shift +9 / +7 mit File-Maske gegen Wrap), schwarze Bauern SE/SW.
#[inline]
fn pawn_attacks_of(pawns: BitBoard, us: Color) -> BitBoard {
    const NOT_A_FILE: u64 = 0xFEFE_FEFE_FEFE_FEFE;
    const NOT_H_FILE: u64 = 0x7F7F_7F7F_7F7F_7F7F;
    let bb: u64 = pawns.0;
    let attacks = match us {
        Color::White => ((bb << 9) & NOT_A_FILE) | ((bb << 7) & NOT_H_FILE),
        Color::Black => ((bb >> 7) & NOT_A_FILE) | ((bb >> 9) & NOT_H_FILE),
    };
    BitBoard::new(attacks)
}

/// Safe Mobility je Figurentyp (Springer, Läufer, Turm, Dame).
/// Zielfelder, die (a) eine eigene Figur belegen oder (b) von einem
/// gegnerischen Bauern angegriffen werden, zählen nicht mit.
/// Rückgabe: (mg, eg) Beitrag aus Sicht von `us`.
fn mobility_score<B: EngineBoard>(board: &B, us: Color, p: &EvalParams) -> (i32, i32) {
    let our_bb = *board.color_combined(us);
    let their_pawns = *board.pieces(Piece::Pawn) & *board.color_combined(!us);
    let occ = *board.combined();
    let safe = !(our_bb | pawn_attacks_of(their_pawns, !us));

    let mut mg = 0;
    let mut eg = 0;

    for sq in *board.pieces(Piece::Knight) & our_bb {
        let n = (get_knight_moves(sq) & safe).popcnt() as i32;
        mg += n * p.knight_mg_mobility;
        eg += n * p.knight_eg_mobility;
        mg += minor_low_mob_penalty(n, p);
    }
    for sq in *board.pieces(Piece::Bishop) & our_bb {
        let n = (get_bishop_moves(sq, occ) & safe).popcnt() as i32;
        mg += n * p.bishop_mg_mobility;
        eg += n * p.bishop_eg_mobility;
        mg += minor_low_mob_penalty(n, p);
    }
    for sq in *board.pieces(Piece::Rook) & our_bb {
        let n = (get_rook_moves(sq, occ) & safe).popcnt() as i32;
        mg += n * p.rook_mg_mobility;
        eg += n * p.rook_eg_mobility;
    }
    for sq in *board.pieces(Piece::Queen) & our_bb {
        let attacks = get_rook_moves(sq, occ) | get_bishop_moves(sq, occ);
        let n = (attacks & safe).popcnt() as i32;
        mg += n * p.queen_mg_mobility;
        eg += n * p.queen_eg_mobility;
    }

    (mg, eg)
}

/// Staffel-Malus fuer vergrabene Leichtfiguren (960-Lookback 11.08.2026).
///
/// `n` = Anzahl der SAFE-Zielfelder der Figur (siehe `mobility_score`).
/// Die konfigurierte Liste wird mit `n` indiziert: Index 0 trifft eine
/// voellig eingemauerte Figur, dahinter flacht der Malus ab; ab
/// `n >= len` gibt es keinen Abzug mehr. Der Wert fliesst NUR in den
/// MG-Pol ein — im Endspiel regelt die lineare eg-Mobility, und ein
/// zusaetzlicher Malus wuerde dort z. B. den "schlechten Laeufer" in
/// Remis-Festungen doppelt bestrafen.
///
/// Leere Liste (Code-Default) → immer 0, Eval bit-exakt wie vorher.
#[inline]
fn minor_low_mob_penalty(n: i32, p: &EvalParams) -> i32 {
    *p.minor_low_mob_penalty_mg.get(n as usize).unwrap_or(&0)
}

/// König-Aktivitäts-Bonus im Endspiel (aus Sicht von Weiß).
/// Positiv wenn weißer König zentraler steht als schwarzer.
/// Skaliert linear mit dem "Endspielgrad" (phase sinkt → Bonus steigt).
///
/// Guard (2026-04-15, verschärft 2026-04-16): Der Bonus je Seite wird
/// unterdrückt, solange der Gegner ein realistisches Mattnetz weben kann.
/// Ursprünglich nur Dame oder 2 Türme — die 16.04-Analyse zeigte aber
/// endgame `allows_mate`-Fälle mit R+Minor (z.B. Martuni vs WolfuhfuhBot
/// 40.Kc2, simbelmyne 41.Kc2). Deshalb triggert der Guard jetzt auch bei
/// Turm + Leichtfigur. KRvK, KBvK, KNvK und KBBvK bleiben unberührt.
fn king_activity_endgame<B: EngineBoard>(board: &B, phase: i32, p: &EvalParams) -> i32 {
    if phase >= p.king_activity_phase_threshold {
        return 0;
    }
    let w = if heavy_piece_threat(board, Color::Black) {
        0
    } else {
        king_centralization_score(board.king_square(Color::White))
    };
    let b = if heavy_piece_threat(board, Color::White) {
        0
    } else {
        king_centralization_score(board.king_square(Color::Black))
    };
    let eg_weight = p.king_activity_phase_threshold - phase;
    (w - b) * eg_weight * p.king_activity_bonus / p.king_activity_phase_threshold
}

/// Bedrohung für den gegnerischen König durch Mattmaterial von `side`:
/// Dame, zwei Türme oder ein Turm + mindestens eine Leichtfigur.
fn heavy_piece_threat<B: EngineBoard>(board: &B, side: Color) -> bool {
    let side_bb = *board.color_combined(side);
    let queens = (*board.pieces(Piece::Queen) & side_bb).popcnt();
    if queens > 0 {
        return true;
    }
    let rooks = (*board.pieces(Piece::Rook) & side_bb).popcnt();
    if rooks >= 2 {
        return true;
    }
    if rooks >= 1 {
        let minors =
            ((*board.pieces(Piece::Bishop) | *board.pieces(Piece::Knight)) & side_bb).popcnt();
        if minors >= 1 {
            return true;
        }
    }
    false
}

/// Zentralisierungswert eines Feldes: 7 = Zentrum (d4/d5/e4/e5), 0 = Ecke.
/// Manhattan-Abstand zur Zentrums-2x2-Box (d4, d5, e4, e5).
#[inline]
fn king_centralization_score(sq: Square) -> i32 {
    let file = sq.get_file().to_index() as i32;
    let rank = sq.get_rank().to_index() as i32;
    // file 3-4 = d/e-Linie, rank 3-4 = Reihe 4/5.
    let file_dist = (file - 3).abs().min((file - 4).abs());
    let rank_dist = (rank - 3).abs().min((rank - 4).abs());
    7 - file_dist - rank_dist
}

// =========================================================================
// Debug-Breakdown: parallel zu evaluate(), aber legt jeden Term einzeln ab.
// Wird NICHT vom Hot-Path aufgerufen — nur ueber das UCI-Kommando `eval`.
// =========================================================================

#[derive(Default, Debug, Clone)]
pub struct PerSideBreakdown {
    pub material: i32,
    pub pawn_bonus: i32,
    pub knight_backrank: i32,
    /// Knight-Outpost-Bonus (bereits phase-getapert, summiert ueber alle
    /// Outpost-Springer der Seite). Siehe `is_knight_outpost`.
    pub knight_outpost: i32,
    /// Entwicklungs-Malus fuer Laeufer auf der eigenen Grundreihe
    /// (bereits phase-getapert, summiert). Siehe `bishop_backrank_penalty_mg`.
    pub bishop_backrank: i32,
    pub bishop_pair: i32,
    pub connected_rooks: i32,
    pub rook_file: i32,
    pub rook_passed: i32,
    pub rook_seventh: i32,
    pub phalanx: i32,
    pub king_safety: i32,
    pub pst_mg: i32,
    pub pst_eg: i32,
    pub mobility_mg: i32,
    pub mobility_eg: i32,
    pub rook_trapped_eg: i32,
}

#[derive(Default, Debug, Clone)]
pub struct EvalBreakdown {
    pub endgame_override: Option<i32>,
    pub phase: i32,
    pub white: PerSideBreakdown,
    pub black: PerSideBreakdown,
    pub pst_tapered: i32,
    pub mobility_tapered: i32,
    pub rook_trapped_tapered: i32,
    pub king_activity_eg: i32,
    pub king_passed_synergy: i32,
    /// Diff (Weiss - Schwarz) des Pawn-Endgame-Guard, bereits phase-getapert.
    /// Inaktiv (=0), wenn das NPM-Gate verletzt wird oder die Phase
    /// >= `king_activity_phase_threshold` liegt.
    pub pawn_endgame_guard: i32,
    /// Per-Seite-Rohwerte des Guards (vor Phase-Tapering, vor Diff).
    /// Nur fuer Debug-Ausgabe und Tests.
    pub white_endgame_guard_raw: i32,
    pub black_endgame_guard_raw: i32,
    /// Material-Imbalance-Term (Weiss-Sicht, phase-getapert). Siehe `material_imbalance`.
    pub imbalance: i32,
    /// Material-Defizit-Daempfung (Weiss-Sicht). Siehe `material_deficit_damping`.
    /// Negativ = Weiss bekommt einen Kompensations-Abschlag, positiv = Schwarz.
    pub damping: i32,
    pub total: i32,
}

/// Spiegelt evaluate_side, schreibt aber jeden Beitrag separat in eine
/// PerSideBreakdown statt sie zu einer Summe zu addieren.
fn evaluate_side_breakdown<B: EngineBoard>(
    board: &B,
    us: Color,
    p: &EvalParams,
    phase: i32,
) -> PerSideBreakdown {
    let mut b = PerSideBreakdown::default();

    let our_bb = *board.color_combined(us);
    let their_pawns = *board.pieces(Piece::Pawn) & *board.color_combined(!us);
    let our_pawns = *board.pieces(Piece::Pawn) & our_bb;
    let own_pawn_count = our_pawns.popcnt() as i32;
    let total_pawn_count = own_pawn_count + their_pawns.popcnt() as i32;
    let home_rank = match us {
        Color::White => Rank::First,
        Color::Black => Rank::Eighth,
    };

    for sq in our_bb {
        let Some(piece) = board.piece_on(sq) else {
            continue;
        };
        b.material += piece_material(piece, p, phase, own_pawn_count, total_pawn_count);

        match piece {
            Piece::Pawn => b.pawn_bonus += pawn_bonus(sq, us, our_pawns, their_pawns, p),
            Piece::Knight => {
                let rank = sq.get_rank();
                if rank == Rank::First || rank == Rank::Eighth {
                    b.knight_backrank += p.knight_backrank_penalty;
                }
                if is_knight_outpost(sq, us, our_pawns, their_pawns) {
                    b.knight_outpost += taper(p.outpost_knight_mg, p.outpost_knight_eg, phase);
                }
            }
            Piece::Bishop => {
                if sq.get_rank() == home_rank {
                    b.bishop_backrank += taper(p.bishop_backrank_penalty_mg, 0, phase);
                }
            }
            _ => {}
        }
    }

    // Step-3-Spiegelung der `evaluate_side`-Logik fuer den Debug-Breakdown.
    let our_bishops = *board.pieces(Piece::Bishop) & our_bb;
    if our_bishops.popcnt() >= 2 {
        let eg_value = p.bishop_pair_eg + (16 - total_pawn_count) * p.bp_open_scale;
        b.bishop_pair = taper(p.bishop_pair_mg, eg_value, phase);
    }

    let our_rooks = *board.pieces(Piece::Rook) & our_bb;
    if rooks_connected(board, our_rooks) {
        b.connected_rooks = p.connected_rooks_pair;
    }
    b.rook_file = rook_file_bonus(our_rooks, our_pawns, their_pawns, p);
    b.rook_passed = rook_passed_pawn_bonus(us, our_rooks, our_pawns, their_pawns, p);
    b.rook_seventh = rook_seventh_rank_bonus(board, us, our_rooks, p);
    b.phalanx = phalanx_bonus(our_pawns, p);
    b.king_safety = king_safety_with_phase(board, us, p, phase);

    let (mg, eg) = pst_score(board, us);
    b.pst_mg = mg;
    b.pst_eg = eg;

    let (mob_mg, mob_eg) = mobility_score(board, us, p);
    b.mobility_mg = mob_mg;
    b.mobility_eg = mob_eg;

    b.rook_trapped_eg = rook_trapped_endgame_malus(board, us, p);

    b
}

/// Liefert eine komponentenweise Aufschluesselung der Stellungsbewertung.
/// Das Total stimmt mit `evaluate()` ueberein (Sanity-Check unten).
pub fn evaluate_breakdown<B: EngineBoard>(board: &B, p: &EvalParams) -> EvalBreakdown {
    let mut bd = EvalBreakdown::default();
    bd.phase = game_phase(board);

    if let Some(s) = endgame::endgame_score(board, p) {
        bd.endgame_override = Some(s);
        bd.total = s;
        return bd;
    }

    bd.white = evaluate_side_breakdown(board, Color::White, p, bd.phase);
    bd.black = evaluate_side_breakdown(board, Color::Black, p, bd.phase);

    bd.pst_tapered = taper(
        bd.white.pst_mg - bd.black.pst_mg,
        bd.white.pst_eg - bd.black.pst_eg,
        bd.phase,
    );
    bd.mobility_tapered = taper(
        bd.white.mobility_mg - bd.black.mobility_mg,
        bd.white.mobility_eg - bd.black.mobility_eg,
        bd.phase,
    );
    bd.rook_trapped_tapered = taper(
        0,
        bd.white.rook_trapped_eg - bd.black.rook_trapped_eg,
        bd.phase,
    );
    bd.king_activity_eg = king_activity_endgame(board, bd.phase, p);
    bd.king_passed_synergy = king_passed_pawn_synergy(board, bd.phase, p);

    // Pawn-Endgame-Guard: Roh-Beitraege immer berechnen (fuer Debug-
    // Sichtbarkeit), die phasen-getaperte Diff aber nur, wenn das Gate
    // und der Phase-Threshold das auch zulassen.
    if is_simple_pawn_endgame(board, p) {
        bd.white_endgame_guard_raw = side_pawn_endgame_guard(board, Color::White, p);
        bd.black_endgame_guard_raw = side_pawn_endgame_guard(board, Color::Black, p);
    }
    bd.pawn_endgame_guard = pawn_endgame_guard(board, bd.phase, p);
    bd.imbalance = material_imbalance(board, bd.phase, p);
    // Gleiche Roh-PST-eg-Summen wie evaluate() (hier aus dem Breakdown), damit
    // beide Pfade denselben Daempfungswert ergeben (Konsistenz-Check unten).
    bd.damping = material_deficit_damping(board, bd.phase, p, bd.white.pst_eg, bd.black.pst_eg);

    // Summe aller Per-Side-Terme (so wie evaluate_side sie addiert), als Differenz.
    let side_sum_white = bd.white.material
        + bd.white.pawn_bonus
        + bd.white.knight_backrank
        + bd.white.knight_outpost
        + bd.white.bishop_backrank
        + bd.white.bishop_pair
        + bd.white.connected_rooks
        + bd.white.rook_file
        + bd.white.rook_passed
        + bd.white.rook_seventh
        + bd.white.phalanx
        + bd.white.king_safety;
    let side_sum_black = bd.black.material
        + bd.black.pawn_bonus
        + bd.black.knight_backrank
        + bd.black.knight_outpost
        + bd.black.bishop_backrank
        + bd.black.bishop_pair
        + bd.black.connected_rooks
        + bd.black.rook_file
        + bd.black.rook_passed
        + bd.black.rook_seventh
        + bd.black.phalanx
        + bd.black.king_safety;
    let non_pst = side_sum_white - side_sum_black;

    bd.total = non_pst
        + bd.pst_tapered
        + bd.king_activity_eg
        + bd.king_passed_synergy
        + bd.pawn_endgame_guard
        + bd.mobility_tapered
        + bd.rook_trapped_tapered
        + bd.imbalance
        + bd.damping;
    bd
}

/// Druckt die EvalBreakdown als `info string`-Zeilen — UCI-konform, sodass
/// das Kommando `eval` aus jedem UCI-Tool / Skript sauber gelesen werden kann.
/// Vorzeichen-Konvention: Diff = White - Black (positiv = gut fuer Weiss).
pub fn print_eval_breakdown<B: EngineBoard>(board: &B, p: &EvalParams) {
    let bd = evaluate_breakdown(board, p);

    println!("info string ===== eval breakdown (Sicht: Weiss) =====");
    if let Some(eg) = bd.endgame_override {
        println!("info string ENDGAME-OVERRIDE aktiv: {} cp", eg);
        println!("info string total                {:>6} cp", bd.total);
        return;
    }
    println!("info string phase: {} / {}", bd.phase, MAX_PHASE);

    let line = |name: &str, w: i32, b: i32| {
        println!(
            "info string {:>22}  W={:>6}  B={:>6}  Diff={:>6}",
            name,
            w,
            b,
            w - b
        );
    };
    line("material", bd.white.material, bd.black.material);
    line("pawn_bonus", bd.white.pawn_bonus, bd.black.pawn_bonus);
    line(
        "knight_backrank",
        bd.white.knight_backrank,
        bd.black.knight_backrank,
    );
    line(
        "knight_outpost",
        bd.white.knight_outpost,
        bd.black.knight_outpost,
    );
    line(
        "bishop_backrank",
        bd.white.bishop_backrank,
        bd.black.bishop_backrank,
    );
    line("bishop_pair", bd.white.bishop_pair, bd.black.bishop_pair);
    line(
        "connected_rooks",
        bd.white.connected_rooks,
        bd.black.connected_rooks,
    );
    line("rook_file", bd.white.rook_file, bd.black.rook_file);
    line("rook_passed", bd.white.rook_passed, bd.black.rook_passed);
    line("rook_seventh", bd.white.rook_seventh, bd.black.rook_seventh);
    line("phalanx", bd.white.phalanx, bd.black.phalanx);
    line("king_safety", bd.white.king_safety, bd.black.king_safety);
    line("pst_mg", bd.white.pst_mg, bd.black.pst_mg);
    line("pst_eg", bd.white.pst_eg, bd.black.pst_eg);
    line("mobility_mg", bd.white.mobility_mg, bd.black.mobility_mg);
    line("mobility_eg", bd.white.mobility_eg, bd.black.mobility_eg);
    line(
        "rook_trapped_eg",
        bd.white.rook_trapped_eg,
        bd.black.rook_trapped_eg,
    );
    // Pawn-Endgame-Guard: Roh-Beitraege pro Seite (vor Phase-Skalierung).
    // Der getaperte Diff steht weiter unten unter "abgeleitete Terme".
    line(
        "pawn_endgame_guard",
        bd.white_endgame_guard_raw,
        bd.black_endgame_guard_raw,
    );

    println!("info string ----- abgeleitete Terme (Weiss-Sicht) -----");
    println!("info string pst_tapered          {:>6}", bd.pst_tapered);
    println!(
        "info string mobility_tapered     {:>6}",
        bd.mobility_tapered
    );
    println!(
        "info string rook_trapped_tapered {:>6}",
        bd.rook_trapped_tapered
    );
    println!(
        "info string king_activity_eg     {:>6}",
        bd.king_activity_eg
    );
    println!(
        "info string king_passed_synergy  {:>6}",
        bd.king_passed_synergy
    );
    println!(
        "info string pawn_eg_guard_taper  {:>6}",
        bd.pawn_endgame_guard
    );
    println!("info string imbalance            {:>6}", bd.imbalance);
    println!("info string damping              {:>6}", bd.damping);
    println!("info string total                {:>6} cp", bd.total);

    // Sanity: muss mit evaluate() uebereinstimmen.
    let direct = evaluate(board, p);
    if direct != bd.total {
        println!(
            "info string WARN breakdown.total ({}) != evaluate() ({}) — Refactor noetig",
            bd.total, direct
        );
    }
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
    fn knight_outpost_detected_and_scored() {
        // Weisser Springer d5, gedeckt vom Bauern c4; schwarze c/e-Bauern fehlen
        // (Loch) → d5 ist ein echter Outpost. Der schwarze d6-Bauer steht auf der
        // EIGENEN Linie des Springers und kann ihn diagonal nicht angreifen.
        let b = Board::from_str("4k3/8/3p4/3N4/2P5/8/8/4K3 w - - 0 1").unwrap();
        let wp = *b.pieces(Piece::Pawn) & *b.color_combined(Color::White);
        let bp = *b.pieces(Piece::Pawn) & *b.color_combined(Color::Black);
        assert!(is_knight_outpost(
            Square::from_str("d5").unwrap(),
            Color::White,
            wp,
            bp
        ));

        // Default 0 → kein Effekt; aktiver Bonus → Weiss-Vorteil steigt.
        let p0 = EvalParams::default();
        let mut p1 = EvalParams::default();
        p1.outpost_knight_mg = 30;
        p1.outpost_knight_eg = 20;
        assert!(evaluate(&b, &p1) > evaluate(&b, &p0));
    }

    #[test]
    fn material_deficit_damping_cuts_compensation() {
        // Bxe4-Illusions-Blatt (Diagnose 06.06.2026): Weiss ist eine Leichtfigur
        // fuer einen Bauern hinten (statisches Defizit 200), hat aber einen
        // vorgeschobenen c6-Freibauer + positives pst_eg als "Kompensation".
        let b = Board::from_str("r1b2rk1/p4p2/2P3pp/8/8/P1Q1P1R1/1P3P1q/2R1K3 w - - 0 1").unwrap();

        // Default (Prozentsaetze 100) → Daempfung ist ein No-op, Eval unveraendert.
        let p0 = EvalParams::default();
        assert_eq!(
            material_deficit_damping(&b, game_phase(&b), &p0, 70, -70),
            0
        );

        // Aktive Daempfung (50/50 ab Defizit 200): Weiss als unterlegene Seite
        // bekommt seinen Freibauer- und PST-eg-Ueberschuss gekuerzt → Eval faellt.
        let mut p1 = EvalParams::default();
        p1.damp_deficit_threshold = 200;
        p1.damp_passed_pct = 50;
        p1.damp_pst_eg_pct = 50;
        assert!(evaluate(&b, &p1) < evaluate(&b, &p0));

        // Breakdown muss mit evaluate() uebereinstimmen (Konsistenz-Check).
        assert_eq!(evaluate_breakdown(&b, &p1).total, evaluate(&b, &p1));

        // Materielle Gleichheit (Startstellung): Schwelle nie erreicht → 0,
        // selbst bei aktiven Prozentsaetzen.
        let start = Board::default();
        assert_eq!(
            material_deficit_damping(&start, game_phase(&start), &p1, 0, 0),
            0
        );
    }

    #[test]
    fn knight_outpost_requires_support_and_hole() {
        let pawns = |b: &Board| {
            (
                *b.pieces(Piece::Pawn) & *b.color_combined(Color::White),
                *b.pieces(Piece::Pawn) & *b.color_combined(Color::Black),
            )
        };
        let d5 = Square::from_str("d5").unwrap();

        // (a) ungedeckt (kein c4/e4-Bauer) → kein Outpost
        let b1 = Board::from_str("4k3/8/8/3N4/8/8/8/4K3 w - - 0 1").unwrap();
        let (wp1, bp1) = pawns(&b1);
        assert!(!is_knight_outpost(d5, Color::White, wp1, bp1));

        // (b) gedeckt, aber schwarzer e6-Bauer greift d5 an (e-Linie, davor)
        //     → kein Loch → kein Outpost
        let b2 = Board::from_str("4k3/8/4p3/3N4/2P5/8/8/4K3 w - - 0 1").unwrap();
        let (wp2, bp2) = pawns(&b2);
        assert!(!is_knight_outpost(d5, Color::White, wp2, bp2));
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
        // Non-PST white: 300 (material) + 25 (file) + 90 (3 × passed rank-2 à 30cp) + 30 (phalanx) - 30 (centre king) = 415
        // Non-PST black: -30 (centre king) → non_pst total: 415 - (-30) = 445
        // PST diff (phase=0): Bauern d4/e4/f4 eg = 60, Kings cancel → +60
        // King-Freibauer-Synergie (phase=0 → voller Effekt):
        //   Weiß (Ke1): 3 × chebyshev(e1,{d4,e4,f4})=3, bonus 2 → 3×(7−3)×2 = 24
        //   Schwarz (Ke8) als Blocker gegen Weiß-Freibauern: chebyshev={4,4,4}, bonus 3 → 3×(7−4)×3 = 27
        //   Diff: 24 − 27 = −3. Macht Sinn: Schwarzer König ist bereits auf der Rückseite
        //   der Bauernkette, der weiße nicht — als Blocker näher dran als der weiße als Begleiter.
        // Total: 505 − 3 = 502
        assert_eq!(evaluate(&b, &p), 502);
    }

    #[test]
    fn bishop_pair_and_backrank_knight() {
        // Weiss hat Laeuferpaar und einen Springer auf b1
        let b = Board::from_str("4k3/8/8/8/8/8/8/1NB1KB2 w - - 0 1").unwrap();
        let p = EvalParams::default();
        // Non-PST: 900 + 30 (pair) - 50 (backrank knight) = 880
        // PST: die Grundreihen-Figuren werden stark abgewertet → taper bei phase=3
        // Mobility (phase=3, keine Bauern → safe = nicht own):
        //   Nb1: 3 Zuege (a3, c3, d2)         → mg 9, eg 9
        //   Bc1: 7 Zuege (NE 5 + NW 2)        → mg 21, eg 28
        //   Bf1: 7 Zuege (NE 2 + NW 5)        → mg 21, eg 28
        //   Summe Weiss: mg 51, eg 65. Schwarz nur Koenig → 0.
        //   taper(51, 65, 3) = (51*3 + 65*21)/24 = 1518/24 = 63
        // Total: 820 + 63 = 883
        assert_eq!(evaluate(&b, &p), 883);
    }

    #[test]
    fn connected_rooks() {
        // KRRvK ist mittlerweile Mop-up — Endspielmodul liefert die Bewertung
        // (Material + Eckenterm + Koenigsnaehe).
        let b = Board::from_str("3k4/8/8/8/4K3/8/8/R6R w - - 0 1").unwrap();
        let p = EvalParams::default();
        // 1000 (2 Tuerme) + 20*center_manhattan_distance + 10*(14-2*4) Koenigsnaehe.
        // Mop-up nutzt jetzt die Zentrums-Distanz (CPW) statt Chebyshev-zur-Ecke:
        // schwacher Koenig d8 → df(d)=0, dr(8)=3 → CMD=3, Eckenterm 20*3 = 60.
        // = 1000 + 60 + 60 = 1120
        assert_eq!(evaluate(&b, &p), 1120);
    }

    #[test]
    fn rooks_not_connected_when_blocked() {
        // Laeufer auf d1 blockt die Verbindung zwischen a1 und h1
        let b = Board::from_str("3k4/8/8/8/4K3/8/8/R2B3R w - - 0 1").unwrap();
        let p = EvalParams::default();
        // Non-PST: 1300 + 30 (schwarzer Koenig center) + 60 (2 Tuerme offene Linien) = 1390
        // PST taper bei phase=5: +5
        // king_activity_endgame: phase=5 < threshold=16, W-e4 score=7, B-d8 score=4,
        //   eg_weight=11. Guard (2026-04-15): Weiß hat 2 Türme → Schwarz-Bonus
        //   unterdrückt (b=0). Weiß ungefährdet (Schwarz hat nichts). bonus = 7*11*3/16 = 14
        // Mobility (keine Bauern → safe = nicht own):
        //   Ra1: 10 Attacks (7N + 3E bis d1), -1 own d1 = 9      → mg 18, eg 45
        //   Bd1: 7 Zuege (NE 4 + NW 3)                           → mg 21, eg 28
        //   Rh1: 11 Attacks (7N + 4W bis d1), -1 own d1 = 10     → mg 20, eg 50
        //   Summe Weiss: mg 59, eg 123. Schwarz 0.
        //   taper(59, 123, 5) = (59*5 + 123*19)/24 = 2632/24 = 109
        // Total: 1390 + 5 + 14 + 109 = 1518
        assert_eq!(evaluate(&b, &p), 1518);
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
    fn ks_exposure_midgame_central_king() {
        // Schwarzer König zentral e5 (rank 4, home=7 → rank_dist=3).
        // Weiß hat 2R + 2N = 1600cp NPM (über 1500-Schwelle).
        // Phase: 2*2 (R) + 2*1 (N) = 6 — schon Endspiel-Übergang.
        // exposure = (3-2) * 1600 / 1000 = 1 (Integer-Div)
        // penalty  = 1 * 12 * 6 / 16 = 4 cp
        // Schild=0 (König nicht auf Heim), Angreifer=0 (keine erreicht
        // d4-f4/d5-f5/d6-f6) → king_safety = 0 - 0 - 4 = -4.
        // Der kleine Wert ist gewollt: in dieser Phase soll der Term
        // schon stark gedämpft sein.
        let b = Board::from_str("8/8/8/4k3/8/8/8/RN2K1NR w - - 0 1").unwrap();
        let p = EvalParams::default();
        assert_eq!(king_safety(&b, Color::Black, &p), -4);
    }

    #[test]
    fn ks_exposure_suppressed_when_low_enemy_material() {
        // Gleiche König-Stellung (e5), aber Weiß hat nur noch einen Turm:
        // NPM = 500cp, unter der 1500cp-Schwelle → kein Expositions-Term.
        // shield=0 (nicht auf Heimrand), danger=0 (keine Angreifer in Zone),
        // exposure=0 → king_safety = 0.
        let b = Board::from_str("8/8/8/4k3/8/8/8/R3K3 w - - 0 1").unwrap();
        let p = EvalParams::default();
        assert_eq!(king_safety(&b, Color::Black, &p), 0);
    }

    #[test]
    fn ks_exposure_home_rank_king_not_penalized() {
        // Rochierter schwarzer König auf g8 (rank 7 = Heimrand → rank_dist=0).
        // Weiß hat 2R + N + B = 1600cp NPM (über der 1500-Schwelle) — alle
        // Figuren weit weg auf der Grundreihe, keine greift g8-Zone an.
        // Der Expositions-Term darf hier trotz hohem Gegner-Material NICHT
        // feuern, weil rank_dist < 3. Genau das prüfen wir.
        // shield: 3 Linien f/g/h leer → 3 × -15 = -45
        // danger: keine Figur in der 3×3-Zone → 0
        // exposure: rank_dist=0 → 0
        // king_safety = -45 - 0 - 0 = -45
        let b = Board::from_str("6k1/8/8/8/8/8/8/RRNBK3 w - - 0 1").unwrap();
        let p = EvalParams::default();
        assert_eq!(king_safety(&b, Color::Black, &p), -45);
    }

    #[test]
    fn ks_exposure_rank_dist_2_not_penalized_anymore() {
        // Schwarzer König auf e6 (rank 5, home=7 → rank_dist=2).
        // Mit der 26.04-Anpassung soll rank_dist=2 noch nicht greifen
        // (Schwelle ist jetzt rank_dist >= 3) — vorher hätte hier ein
        // Malus von ~20cp gestanden.
        // Setup: 2R + 2N + 1B = 1900cp (≥1500), aber so, dass keine
        // Figur die Zone {d5..f7} angreift.
        // (Bc1 hat Diagonale c1–h6, die kein Zonenfeld trifft; Nb1/Ng1
        // attackieren a3/c3/d2/e2/f3/h3 — auch keins in der Zone.)
        let b = Board::from_str("8/8/4k3/8/8/8/8/RNB1K1NR w - - 0 1").unwrap();
        let p = EvalParams::default();
        assert_eq!(king_safety(&b, Color::Black, &p), 0);
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

    #[test]
    fn mobility_central_knight_has_eight_safe_squares() {
        // Springer auf d4, keine Bauern → alle 8 Zielfelder safe
        let b = Board::from_str("4k3/8/8/8/3N4/8/8/4K3 w - - 0 1").unwrap();
        let p = EvalParams::default();
        let (mg, eg) = mobility_score(&b, Color::White, &p);
        // knight_mg=3, knight_eg=3
        assert_eq!((mg, eg), (24, 24));
        // Schwarz hat nur den Koenig → 0
        let (bmg, beg) = mobility_score(&b, Color::Black, &p);
        assert_eq!((bmg, beg), (0, 0));
    }

    #[test]
    fn mobility_enemy_pawn_masks_target_square() {
        // Springer auf f3; schwarzer Bauer auf d6 deckt e5.
        // get_knight_moves(f3) = {e1, g1, d2, h2, d4, h4, e5, g5}.
        //   e1 ist eigener Koenig → raus (own-Maske).
        //   e5 ist von d6-Bauer angegriffen → raus (safe-Maske).
        //   Bleibt 6 safe Zuege.
        let b = Board::from_str("4k3/8/3p4/8/8/5N2/8/4K3 w - - 0 1").unwrap();
        let p = EvalParams::default();
        let (mg, eg) = mobility_score(&b, Color::White, &p);
        assert_eq!((mg, eg), (18, 18));
    }

    // ---- Regel A: Tarrasch (Turm + Freibauer) ----

    #[test]
    fn rook_behind_own_passed_pawn_is_bonus() {
        // Weißer Freibauer a6, weißer Turm a1 hinter ihm (kleinere Reihe =
        // hinter dem weißen Bauer). Schwarz nur König h8.
        let behind = Board::from_str("7k/8/P7/8/8/8/8/R3K3 w - - 0 1").unwrap();
        // Gleiche Stellung, aber Turm auf a7 (vor dem Bauer).
        let ahead = Board::from_str("7k/R7/P7/8/8/8/8/4K3 w - - 0 1").unwrap();
        let p = EvalParams::default();
        let bonus = rook_passed_pawn_bonus(
            Color::White,
            *behind.pieces(Piece::Rook) & *behind.color_combined(Color::White),
            *behind.pieces(Piece::Pawn) & *behind.color_combined(Color::White),
            *behind.pieces(Piece::Pawn) & *behind.color_combined(Color::Black),
            &p,
        );
        let penalty = rook_passed_pawn_bonus(
            Color::White,
            *ahead.pieces(Piece::Rook) & *ahead.color_combined(Color::White),
            *ahead.pieces(Piece::Pawn) & *ahead.color_combined(Color::White),
            *ahead.pieces(Piece::Pawn) & *ahead.color_combined(Color::Black),
            &p,
        );
        assert_eq!(bonus, p.rook_behind_own_passed_bonus);
        assert_eq!(penalty, p.rook_blocks_own_passed_penalty);
    }

    #[test]
    fn rook_behind_enemy_passed_pawn_is_bonus() {
        // Schwarzer Freibauer a3, weißer Turm a1 hinter ihm aus Sicht des
        // schwarzen Bauers (der nach Süden läuft → Norden ist hinten).
        // Weißer Turm auf a1 ist kleinere Reihe als a3 — das ist aus
        // Schwarz-Bauer-Sicht "vor" ihm. NICHT der gewünschte Fall.
        // Für den Bonus muss der weiße Turm AUF größerer Reihe stehen als
        // der schwarze Bauer. Also a6 hinter a3 (aus Schwarz-Sicht).
        let b = Board::from_str("7k/8/R7/8/8/p7/8/4K3 w - - 0 1").unwrap();
        let p = EvalParams::default();
        let score = rook_passed_pawn_bonus(
            Color::White,
            *b.pieces(Piece::Rook) & *b.color_combined(Color::White),
            *b.pieces(Piece::Pawn) & *b.color_combined(Color::White),
            *b.pieces(Piece::Pawn) & *b.color_combined(Color::Black),
            &p,
        );
        assert_eq!(score, p.rook_behind_enemy_passed_bonus);
    }

    #[test]
    fn rook_unrelated_to_passed_pawn_is_zero() {
        // Turm auf h1, Bauer auf a6. Nicht dieselbe Linie → kein Effekt.
        // Schwarzer König auf g8 (nicht h8), damit der h1-Turm nicht Schach gibt.
        let b = Board::from_str("6k1/8/P7/8/8/8/8/4K2R w - - 0 1").unwrap();
        let p = EvalParams::default();
        let score = rook_passed_pawn_bonus(
            Color::White,
            *b.pieces(Piece::Rook) & *b.color_combined(Color::White),
            *b.pieces(Piece::Pawn) & *b.color_combined(Color::White),
            *b.pieces(Piece::Pawn) & *b.color_combined(Color::Black),
            &p,
        );
        assert_eq!(score, 0);
    }

    // ---- Regel B: Turm auf 7. Reihe ----

    #[test]
    fn rook_on_seventh_rank_bonus() {
        // Weißer Turm auf b7, schwarzer König h8 (nicht auf Grundreihe für
        // diesen Test, um nur den Basis-Bonus zu prüfen). Umweg: König g6.
        // FEN: Turm b7, schwarzer König g6, weißer König e1.
        let b = Board::from_str("8/1R6/6k1/8/8/8/8/4K3 w - - 0 1").unwrap();
        let p = EvalParams::default();
        let score = rook_seventh_rank_bonus(
            &b,
            Color::White,
            *b.pieces(Piece::Rook) & *b.color_combined(Color::White),
            &p,
        );
        assert_eq!(score, p.rook_seventh_bonus);
    }

    #[test]
    fn rook_on_seventh_with_king_on_eighth_extra_bonus() {
        // Weißer Turm b7, schwarzer König auf 8. Reihe (h8) → klassisches
        // abschneidendes Szenario, voller Bonus.
        let b = Board::from_str("7k/1R6/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        let p = EvalParams::default();
        let score = rook_seventh_rank_bonus(
            &b,
            Color::White,
            *b.pieces(Piece::Rook) & *b.color_combined(Color::White),
            &p,
        );
        assert_eq!(
            score,
            p.rook_seventh_bonus + p.rook_seventh_vs_king_eighth_bonus
        );
    }

    #[test]
    fn rook_not_on_seventh_no_bonus() {
        // Turm auf e1, kein 7.-Reihe-Bonus, auch wenn König auf 8. steht.
        let b = Board::from_str("7k/8/8/8/8/8/8/4KR2 w - - 0 1").unwrap();
        let p = EvalParams::default();
        let score = rook_seventh_rank_bonus(
            &b,
            Color::White,
            *b.pieces(Piece::Rook) & *b.color_combined(Color::White),
            &p,
        );
        assert_eq!(score, 0);
    }

    #[test]
    fn rook_seventh_rank_mirrors_for_black() {
        // Schwarzer Turm auf der 2. Reihe (= "7. Reihe aus Schwarz-Sicht"),
        // weißer König auf e1 (Grundreihe für Schwarz).
        let b = Board::from_str("4k3/8/8/8/8/8/1r6/4K3 b - - 0 1").unwrap();
        let p = EvalParams::default();
        let score = rook_seventh_rank_bonus(
            &b,
            Color::Black,
            *b.pieces(Piece::Rook) & *b.color_combined(Color::Black),
            &p,
        );
        assert_eq!(
            score,
            p.rook_seventh_bonus + p.rook_seventh_vs_king_eighth_bonus
        );
    }

    // ---- Regel C: König-Freibauer-Synergie ----

    #[test]
    fn king_near_own_passed_pawn_gives_bonus_in_endgame() {
        // Weißer Freibauer a5, weißer König a4 direkt dahinter. Schwarz nur
        // König h8. Phase=0 (reines Bauernendspiel), eg_weight=16 → voller
        // Effekt.
        let b = Board::from_str("7k/8/8/P7/K7/8/8/8 w - - 0 1").unwrap();
        let p = EvalParams::default();
        // chebyshev(a4,a5)=1 → Weiß-Anteil = (7−1)*2 = 12
        // Schwarz-König h8 als Blocker: chebyshev(h8,a5)=max(7,3)=7 → 0
        // eg_weight/threshold = 16/16 = 1 → Diff = 12
        let score = king_passed_pawn_synergy(&b, game_phase(&b), &p);
        assert_eq!(score, 12);
    }

    #[test]
    fn king_near_enemy_passed_pawn_gives_bonus() {
        // Schwarzer Freibauer a5, weißer König a4 direkt davor als Blocker.
        // Weiß hat keinen eigenen Bauern. Erwartet: weißer König als
        // Blocker-Bonus (*3).
        let b = Board::from_str("7k/8/8/p7/K7/8/8/8 w - - 0 1").unwrap();
        let p = EvalParams::default();
        // Weiß-Block: chebyshev(a4,a5)=1 → (7−1)*3 = 18
        // Schwarz-Block: sein eigener Bauer a5 ist Freibauer (weiße Bauern=0),
        //   König h8, chebyshev(h8,a5)=7 → 0 für eigenen Bauer.
        //   Gegner (weiße) Freibauern: keine → 0.
        // Diff = 18 − 0 = 18, phase=0 → volle Skalierung
        let score = king_passed_pawn_synergy(&b, game_phase(&b), &p);
        assert_eq!(score, 18);
    }

    #[test]
    fn king_pawn_synergy_disabled_in_middlegame() {
        // Gleicher Aufbau wie "king_near_own_passed_pawn…", aber mit 4 Damen
        // auf dem Brett (2 je Seite) → Phase = 16, genau auf der Schwelle.
        // Die Damen sind so platziert, dass sie die gegnerischen Könige nicht
        // angreifen (sonst wäre die Stellung ungültig).
        let b = Board::from_str("5k2/4q3/3q4/P7/K7/3Q4/4Q3/8 w - - 0 1").unwrap();
        let p = EvalParams::default();
        assert!(game_phase(&b) >= p.king_activity_phase_threshold);
        let score = king_passed_pawn_synergy(&b, game_phase(&b), &p);
        assert_eq!(score, 0);
    }

    #[test]
    fn king_pawn_synergy_no_passed_pawns_is_zero() {
        // Weiß und Schwarz haben jeweils einen Bauer, aber beide auf
        // derselben Linie → keiner ist Freibauer.
        let b = Board::from_str("7k/4p3/8/8/8/8/4P3/4K3 w - - 0 1").unwrap();
        let p = EvalParams::default();
        let score = king_passed_pawn_synergy(&b, game_phase(&b), &p);
        assert_eq!(score, 0);
    }

    // ---- Regel C: Turm hinter eigenem Bauer eingemauert (Endspiel) ----

    #[test]
    fn rook_trapped_behind_pawn_returns_penalty() {
        // Weißer Turm a1, weißer Bauer a2 — klassische "eingemauert"-Stellung.
        // Roher Endspielwert (vor Tapern) = -10 cp pro Vorkommen.
        let b = Board::from_str("4k3/8/8/8/8/8/P7/R3K3 w - - 0 1").unwrap();
        let p = EvalParams::default();
        let m = rook_trapped_endgame_malus(&b, Color::White, &p);
        assert_eq!(m, p.rook_trapped_endgame_penalty);
    }

    #[test]
    fn rook_trapped_no_pawn_in_front_no_penalty() {
        // Turm a1, kein eigener Bauer auf a2 → kein Malus.
        let b = Board::from_str("4k3/8/8/8/8/8/8/R3K3 w - - 0 1").unwrap();
        let p = EvalParams::default();
        let m = rook_trapped_endgame_malus(&b, Color::White, &p);
        assert_eq!(m, 0);
    }

    #[test]
    fn rook_trapped_only_in_endgame_phase() {
        // Volle Materialschlacht (phase=24): durch Tapern soll der Term
        // im evaluate-Pfad 0 beitragen, selbst wenn die rohe Heuristik feuert.
        // Wir prüfen das direkt am Tapering: roh wäre -10, getapert mit
        // phase=24 ergibt taper(0, -10, 24) = (0*24 + -10*0)/24 = 0.
        assert_eq!(taper(0, -10, 24), 0);
        // Im reinen Endspiel (phase=0) wird der ganze rohe Wert wirksam.
        assert_eq!(taper(0, -10, 0), -10);
        // Bei phase=12 (Mittendrin): linear interpoliert.
        assert_eq!(taper(0, -10, 12), -5);
    }

    #[test]
    fn rook_trapped_black_mirror() {
        // Schwarzer Turm h8, schwarzer Bauer h7 — gespiegelte Konfiguration.
        let b = Board::from_str("4k2r/7p/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        let p = EvalParams::default();
        let m = rook_trapped_endgame_malus(&b, Color::Black, &p);
        assert_eq!(m, p.rook_trapped_endgame_penalty);
    }

    // -----------------------------------------------------------------
    // Pawn-Endgame-Guard (23.05.2026)
    //
    // Tobias-Style: erst neutrale Defaults pruefen (Term muss 0 sein,
    // damit ohne TOML-Override nichts driftet), dann jeden Sub-Term
    // einzeln mit einer minimal-praeparierten Stellung.
    // -----------------------------------------------------------------

    fn p_guard_active() -> EvalParams {
        // Werte wie eval.toml-Vorschlag, fuer Tests fix einkompiliert.
        let mut p = EvalParams::default();
        p.opposition_bonus = 12;
        p.key_square_bonus_by_rank = [0, 0, 10, 18, 28, 40, 30, 0];
        p.rook_pawn_drawish_penalty = -60;
        p.npm_endgame_gate = 700;
        p
    }

    #[test]
    fn guard_defaults_are_neutral() {
        // Mit Default-Params ist der Guard ueberall 0. Wichtig, damit
        // baseline-Binaries ohne [endgame_guard]-Sektion identisch
        // verhalten zur Pre-Feature-Version.
        let b = Board::from_str("4k3/8/3K4/4P3/8/8/8/8 b - - 0 1").unwrap();
        let p = EvalParams::default();
        let score = pawn_endgame_guard(&b, game_phase(&b), &p);
        assert_eq!(score, 0);
    }

    #[test]
    fn guard_gate_blocks_middlegame() {
        // Mittelspiel mit Tuermen: NPM 1000 pro Seite > Gate 700.
        let b = Board::from_str("r3k2r/pppppppp/8/8/8/8/PPPPPPPP/R3K2R w KQkq - 0 1").unwrap();
        let p = p_guard_active();
        let s = pawn_endgame_guard(&b, game_phase(&b), &p);
        assert_eq!(s, 0, "guard must be inactive above NPM gate");
    }

    #[test]
    fn opposition_direct_white_holds() {
        // Ke6 vs Ke8 direkte Opposition, Schwarz am Zug → Weiss "hat die
        // Opposition". Bonus geht an Weiss.
        let b = Board::from_str("4k3/8/4K3/8/8/8/8/8 b - - 0 1").unwrap();
        let p = p_guard_active();
        let w = opposition_bonus(&b, Color::White, &p);
        let bl = opposition_bonus(&b, Color::Black, &p);
        assert_eq!(w, p.opposition_bonus);
        assert_eq!(bl, 0);
    }

    #[test]
    fn opposition_only_for_side_not_to_move() {
        // Gleiche Geometrie, jetzt aber Weiss am Zug → kein Bonus fuer Weiss.
        // Schwarz haette die Opposition, kriegt sie auch.
        let b = Board::from_str("4k3/8/4K3/8/8/8/8/8 w - - 0 1").unwrap();
        let p = p_guard_active();
        assert_eq!(opposition_bonus(&b, Color::White, &p), 0);
        assert_eq!(opposition_bonus(&b, Color::Black, &p), p.opposition_bonus);
    }

    #[test]
    fn opposition_diagonal() {
        // Ke5 vs Kg7 (Diagonale, ein freies Feld dazwischen auf f6).
        // Schwarz am Zug → Weiss hat diagonale Opposition.
        let b = Board::from_str("8/6k1/8/4K3/8/8/8/8 b - - 0 1").unwrap();
        let p = p_guard_active();
        assert_eq!(opposition_bonus(&b, Color::White, &p), p.opposition_bonus);
    }

    #[test]
    fn opposition_blocked_by_pawn() {
        // Ke4 vs Ke6, Bauer auf e5 dazwischen → keine Opposition.
        let b = Board::from_str("8/8/4k3/4P3/4K3/8/8/8 b - - 0 1").unwrap();
        let p = p_guard_active();
        assert_eq!(opposition_bonus(&b, Color::White, &p), 0);
    }

    #[test]
    fn key_square_white_pawn_e5_king_d6() {
        // Bauer e5 (Rang 5), Koenig d6 = Schluesselfeld (Rang 6,
        // file e±1). Bonus = key_square_bonus_by_rank[rel_rank-1=4] = 28.
        let b = Board::from_str("4k3/8/3K4/4P3/8/8/8/8 b - - 0 1").unwrap();
        let p = p_guard_active();
        let bonus = key_square_bonus(&b, Color::White, &p);
        assert_eq!(bonus, p.key_square_bonus_by_rank[4]);
    }

    #[test]
    fn key_square_skipped_for_rook_pawn() {
        // Rook-Pawn h5 + Koenig g6 — kein Key-Square-Bonus, Rook-Pawn-Korrektur
        // uebernimmt diesen Fall.
        let b = Board::from_str("4k3/8/6K1/7P/8/8/8/8 b - - 0 1").unwrap();
        let p = p_guard_active();
        assert_eq!(key_square_bonus(&b, Color::White, &p), 0);
    }

    #[test]
    fn key_square_doubled_pawns_count_only_advanced() {
        // Bauern e5 + e3 (doppelter Bauer auf e-Linie), Koenig auf d6 —
        // Schluesselfeld fuer e5. Es darf NUR einmal gezaehlt werden
        // (nicht zusaetzlich der Key-Square-Pool von e3).
        let b = Board::from_str("4k3/8/3K4/4P3/8/4P3/8/8 b - - 0 1").unwrap();
        let p = p_guard_active();
        let bonus = key_square_bonus(&b, Color::White, &p);
        assert_eq!(bonus, p.key_square_bonus_by_rank[4]);
    }

    #[test]
    fn rook_pawn_correction_fires_in_promo_corner() {
        // h5 + schwarzer K auf h8 (Chebyshev zum h8-Promo-Eck 0). Penalty
        // greift, Wert negativ.
        let b = Board::from_str("7k/8/6K1/7P/8/8/8/8 b - - 0 1").unwrap();
        let p = p_guard_active();
        let corr = rook_pawn_correction(&b, Color::White, &p);
        assert_eq!(corr, p.rook_pawn_drawish_penalty);
    }

    #[test]
    fn rook_pawn_correction_skips_central_pawn() {
        // e5 ist KEIN Rook-Pawn → Korrektur nicht aktiv.
        let b = Board::from_str("4k3/8/3K4/4P3/8/8/8/8 b - - 0 1").unwrap();
        let p = p_guard_active();
        assert_eq!(rook_pawn_correction(&b, Color::White, &p), 0);
    }

    #[test]
    fn rook_pawn_correction_skips_when_other_pawn_passed() {
        // h-Bauer + zentraler Frei-Bauer → Edge-Case ist nicht "einziger
        // Frei-Bauer ist Rook-Pawn".
        let b = Board::from_str("7k/8/6K1/4P2P/8/8/8/8 b - - 0 1").unwrap();
        let p = p_guard_active();
        assert_eq!(rook_pawn_correction(&b, Color::White, &p), 0);
    }

    #[test]
    fn guard_total_endgame_is_active() {
        // Vereinte Sanity: Schluesselfeld d6 + e5 + Ke8 (Schwarz am Zug).
        // pawn_endgame_guard sollte einen positiven, getaperten Wert
        // fuer Weiss liefern (Phase=0 hier, voller Effekt).
        let b = Board::from_str("4k3/8/3K4/4P3/8/8/8/8 b - - 0 1").unwrap();
        let p = p_guard_active();
        let phase = game_phase(&b);
        assert!(phase < p.king_activity_phase_threshold);
        let s = pawn_endgame_guard(&b, phase, &p);
        assert!(s > 0, "expected positive guard score, got {s}");
    }

    #[test]
    fn guard_phase_above_threshold_returns_zero() {
        // Startpos hat phase=24 (>=threshold). NPM ist auch viel zu hoch,
        // beides blockt den Guard.
        let b = Board::default();
        let p = p_guard_active();
        assert_eq!(pawn_endgame_guard(&b, game_phase(&b), &p), 0);
    }

    // --- Low-Mobility-Malus fuer Leichtfiguren (960-Lookback 11.08.2026) ---

    #[test]
    fn low_mob_penalty_default_inactive() {
        // Code-Default = leere Liste → Malus immer 0, Eval bit-exakt wie vorher.
        let p = EvalParams::default();
        assert_eq!(minor_low_mob_penalty(0, &p), 0);
        assert_eq!(minor_low_mob_penalty(5, &p), 0);
    }

    #[test]
    fn low_mob_penalty_staffel_und_obergrenze() {
        let mut p = EvalParams::default();
        p.minor_low_mob_penalty_mg = vec![-30, -20, -10];
        assert_eq!(minor_low_mob_penalty(0, &p), -30);
        assert_eq!(minor_low_mob_penalty(1, &p), -20);
        assert_eq!(minor_low_mob_penalty(2, &p), -10);
        // Ab n >= Listenlaenge kein Abzug mehr — entwickelte Figur ist "frei".
        assert_eq!(minor_low_mob_penalty(3, &p), 0);
        assert_eq!(minor_low_mob_penalty(8, &p), 0);
    }

    #[test]
    fn buried_bishop_gets_mg_malus_only() {
        // Weisser Laeufer a1 hinter dem eigenen b2-Bauern: 0 Safe-Felder —
        // das 960-Eckenlaeufer-Muster in Reinform.
        let b = Board::from_str("4k3/8/8/8/8/8/1P6/B3K3 w - - 0 1").unwrap();
        let base = EvalParams::default();
        let (mg_before, eg_before) = mobility_score(&b, Color::White, &base);
        let mut p = EvalParams::default();
        p.minor_low_mob_penalty_mg = vec![-30, -20, -10];
        let (mg_after, eg_after) = mobility_score(&b, Color::White, &p);
        assert_eq!(mg_after, mg_before - 30, "0 Safe-Felder → Index 0 = -30");
        assert_eq!(eg_after, eg_before, "Malus wirkt nur im MG-Pol");
    }

    #[test]
    fn startpos_balanced_with_low_mob_penalty() {
        // Beide Seiten symmetrisch vergraben → Malus hebt sich auf, Score 0.
        let b = Board::default();
        let mut p = EvalParams::default();
        p.minor_low_mob_penalty_mg = vec![-30, -20, -10];
        assert_eq!(evaluate(&b, &p), 0);
    }
}
