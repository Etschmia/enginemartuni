use crate::endgame;
use crate::eval_config::EvalParams;
use crate::pst::{
    pst_index, BISHOP_PST, KING_PST, KNIGHT_PST, PAWN_PST, QUEEN_PST, ROOK_PST,
};
use chess::{
    get_adjacent_files, get_bishop_moves, get_file, get_king_moves, get_knight_moves,
    get_rook_moves, BitBoard, Board, Color, File, Piece, Rank, Square,
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

    let king_act = king_activity_endgame(board, phase, p);

    let (w_mob_mg, w_mob_eg) = mobility_score(board, Color::White, p);
    let (b_mob_mg, b_mob_eg) = mobility_score(board, Color::Black, p);
    let mob = taper(w_mob_mg - b_mob_mg, w_mob_eg - b_mob_eg, phase);

    non_pst + taper(mg, eg, phase) + king_act + mob
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

    // Tuerme auf offenen / halb-offenen Linien
    score += rook_file_bonus(our_rooks, our_pawns, their_pawns, p);

    // Bauernphalanx (reihenweise)
    score += phalanx_bonus(our_pawns, p);

    // King Safety (positiv = sicher, negativ = in Gefahr)
    score += king_safety(board, us, p);

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
pub fn king_safety(board: &Board, us: Color, p: &EvalParams) -> i32 {
    let king_sq = board.king_square(us);
    let zone = king_zone(king_sq);

    let shield = pawn_shield_score(board, us, king_sq, p);
    let danger = king_danger(board, us, zone, p);
    let exposure = king_exposure_penalty(board, us, p);

    shield - danger - exposure
}

/// König-Expositions-Strafe (eingeführt 2026-04-22).
///
/// Ergänzt `shield` und `danger` um einen "König ist zu weit vom Heimrand
/// weg, während der Gegner noch Schwergewicht hat"-Term. Motiviert durch
/// das mochi_bot-Spiel (EY25JUSH), in dem Martuni mit 16...Kg4 aus dem
/// Schach in die gegnerischen Figuren hineinlief — Kg6 wäre sicher gewesen.
/// Die bisherige Heuristik aus `shield` (deaktiviert sich abseits der
/// Heimreihe) und `danger` (wertet nur die 3×3-Nahzone) erkannte die
/// Gefahr nicht stark genug: Kg6 und Kg4 bekamen praktisch denselben Score.
///
/// Formel:
/// ```text
///   rank_dist = |rank(König) - home_rank|    (0..7)
///   enemy_npm = Σ Figurenwerte des Gegners (Springer+Läufer+Turm+Dame)
///   Gate: rank_dist >= 2     (König auf Heim- oder vorgerückter Reihe ok)
///   Gate: enemy_npm >= 1500  (sonst aktiver König im Endspiel erwünscht)
///   exposure = (rank_dist - 1) * enemy_npm / 1000
///   penalty  = exposure * king_exposure_weight
/// ```
///
/// Beispiel Mochi 16...Kg4 (schwarz): rank_dist=4, weiß-NPM=1600cp
///   exposure = 3 * 1600 / 1000 = 4 (Integer-Div)
///   penalty  = 4 * 20 = 80cp Abzug
/// Vergleich Kg6: rank_dist=2 → exposure = 1 * 1600 / 1000 = 1 → 20cp Abzug
/// Differenz 60cp sollte in den Leaf-Nodes reichen, um Kg4 klar schlechter
/// als Kg6 einzustufen.
///
/// Rückgabewert ist positiv (als "abzuziehender Malus" im Aufrufer gedacht).
fn king_exposure_penalty(board: &Board, us: Color, p: &EvalParams) -> i32 {
    // Abstand des Königs zu seiner Grundreihe
    let king_sq = board.king_square(us);
    let rank = king_sq.get_rank().to_index() as i32;
    let home_rank = match us {
        Color::White => 0,
        Color::Black => 7,
    };
    let rank_dist = (rank - home_rank).abs();

    // rank_dist < 2: König steht auf Heim- oder erster vorgerückter Reihe.
    // Das ist die normale Rochadeposition oder eine minimal vorgerückte
    // Stellung (z.B. Kf1 nach Bauernverlust) — noch keine Exposition.
    if rank_dist < 2 {
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
    // 1500cp = 3 Leichtfiguren / 1R+2Minor / 2R (minus 2N). Alles darunter
    // sind Material-Endspiele, in denen König-Zentralisierung wichtiger ist.
    if enemy_npm < 1500 {
        return 0;
    }

    // (rank_dist - 1): rank_dist=2 → Faktor 1 (leicht), rank_dist=7 → Faktor 6 (extrem).
    // enemy_npm / 1000 als grobe "Wie viel Schwergewicht hat der Gegner"-Skala.
    // Integer-Arithmetik: bewusst keine Gleitkomma — im Mittelspiel ergibt
    // sich ein Bereich von 1..15 Expositions-Punkten.
    let exposure = (rank_dist - 1) * enemy_npm / 1000;
    exposure * p.king_exposure_weight
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

fn is_isolated(our_pawns: BitBoard, file: File) -> bool {
    (our_pawns & get_adjacent_files(file)) == BitBoard::new(0)
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

/// Bauernangriffe einer Seite als BitBoard. Weiße Bauern schlagen NE/NW
/// (shift +9 / +7 mit File-Maske gegen Wrap), schwarze Bauern SE/SW.
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
fn mobility_score(board: &Board, us: Color, p: &EvalParams) -> (i32, i32) {
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
    }
    for sq in *board.pieces(Piece::Bishop) & our_bb {
        let n = (get_bishop_moves(sq, occ) & safe).popcnt() as i32;
        mg += n * p.bishop_mg_mobility;
        eg += n * p.bishop_eg_mobility;
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
fn king_activity_endgame(board: &Board, phase: i32, p: &EvalParams) -> i32 {
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
fn heavy_piece_threat(board: &Board, side: Color) -> bool {
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
fn king_centralization_score(sq: Square) -> i32 {
    let file = sq.get_file().to_index() as i32;
    let rank = sq.get_rank().to_index() as i32;
    // file 3-4 = d/e-Linie, rank 3-4 = Reihe 4/5.
    let file_dist = (file - 3).abs().min((file - 4).abs());
    let rank_dist = (rank - 3).abs().min((rank - 4).abs());
    7 - file_dist - rank_dist
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
        // Non-PST white: 300 (material) + 25 (file) + 90 (3 × passed rank-2 à 30cp) + 30 (phalanx) - 30 (centre king) = 415
        // Non-PST black: -30 (centre king) → non_pst total: 415 - (-30) = 445
        // PST diff (phase=0): Bauern d4/e4/f4 eg = 60, Kings cancel → +60
        // Total: 505
        assert_eq!(evaluate(&b, &p), 505);
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
        // 1000 (2 Tuerme) + 20*(7-3) Eckenterm + 10*(14-2*4) Koenigsnaehe
        // = 1000 + 80 + 60 = 1140
        assert_eq!(evaluate(&b, &p), 1140);
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
        // Schwarzer König zentral auf e5 (rank 4, home=7 → rank_dist=3).
        // Weiß hat 2R + 2N = 1600cp NPM, also über der 1500cp-Schwelle.
        // Kein Schild (König nicht auf Heimrand), keine Angreifer in der 3×3-Zone.
        // exposure = (3-1) * 1600 / 1000 = 3 (Integer-Div)
        // penalty  = 3 * 20 = 60cp
        // Keine Schild-Bonus/Malus-Beiträge → king_safety = 0 - 0 - 60 = -60
        let b = Board::from_str("8/8/8/4k3/8/8/8/RN2K1NR w - - 0 1").unwrap();
        let p = EvalParams::default();
        assert_eq!(king_safety(&b, Color::Black, &p), -60);
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
        // feuern, weil rank_dist < 2. Genau das prüfen wir.
        // shield: 3 Linien f/g/h leer → 3 × -15 = -45
        // danger: keine Figur in der 3×3-Zone → 0
        // exposure: rank_dist=0 → 0
        // king_safety = -45 - 0 - 0 = -45
        let b = Board::from_str("6k1/8/8/8/8/8/8/RRNBK3 w - - 0 1").unwrap();
        let p = EvalParams::default();
        assert_eq!(king_safety(&b, Color::Black, &p), -45);
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
}
