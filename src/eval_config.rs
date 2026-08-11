use std::env;
use std::fs;
use std::path::PathBuf;
use toml::Value;

#[derive(Debug, Clone)]
pub struct EvalParams {
    // Material
    // Statische Anker-Werte. Bewusst NICHT ersetzt durch dynmat: sie sind
    // weiterhin Referenz für `king_exposure_penalty` (NPM-Schwelle) und
    // `endgame::strong_material` (Endspiel-Klassifizierung). Wer hier den
    // Anker bewegt, verschiebt indirekt auch jene Terme — für die Eval-
    // Kalibrierung 2026 wollen wir das ausdrücklich vermeiden.
    pub pawn: i32,
    pub knight: i32,
    pub bishop: i32,
    pub rook: i32,
    pub queen: i32,

    // Dynamische Springer-/Läufer-Werte für Tapering im Material-Score.
    // Wirken NUR im Per-Figur-Materialbeitrag in `evaluate_side`; die
    // statischen `knight`/`bishop` oben bleiben Anker für andere Terme.
    //
    // Idee (Kaufman 1999): Springer profitieren leicht im Mittelspiel von
    // taktischer Bauern-Kooperation, Läufer im Endspiel von Reichweite und
    // Freibauern-Begleitung. Die N–B-Differenz "kippt" im Spielverlauf.
    //
    // Defaults sind *verhaltensgleich* zur statischen Bewertung (alle 300).
    // Damit ist die Code-Änderung ohne TOML-Override neutral. Erst eine
    // Sektion `[material_dynamic]` in eval.toml aktiviert das Feature
    // tatsächlich messbar — siehe eval.toml für die experimentellen Werte.
    pub knight_mg: i32,
    pub knight_eg: i32,
    pub bishop_mg: i32,
    pub bishop_eg: i32,

    // Schritt 2 der dynamischen Figurenbewertung (Pawn-Adjustment).
    // Formeln in `piece_material`:
    //   knight_adj = (own_pawn_count - 8) * knight_pawn_scale
    //   bishop_adj = (16 - total_pawn_count) * bishop_pawn_scale
    // Springer profitiert von eigenen Bauern (Outpost-Stützen,
    // taktische Bauern-Kooperation) → maximaler Wert bei 8 eigenen Bauern,
    // Malus pro verlorenem eigenem Bauern.
    // Läufer profitiert von offener Gesamt-Stellung (weniger Bauern auf dem
    // Brett, freie Diagonalen) → 0 bei vollem Brett, Bonus mit jedem
    // fehlenden Bauer (egal welche Farbe).
    //
    // Defaults sind 0 — verhaltensgleich zur reinen Step-1-Bewertung. Erst
    // ein eval.toml-Override aktiviert das Pawn-Adjustment messbar.
    pub knight_pawn_scale: i32,
    pub bishop_pawn_scale: i32,

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
    /// Entwicklungs-Malus pro Laeufer auf der EIGENEN Grundreihe, nur
    /// Mittelspiel-Pol (`taper(x, 0, phase)`), negativer Wert.
    /// Eingefuehrt nach dem 960-Lookback 26.07.2026: ohne Buch driftete
    /// Martuni in der Eroeffnung still ab (66 Opening-Blunder in 960 vs. 5
    /// im Standard) — die PSTs allein erzeugen auf den 960-Startaufstellungen
    /// keinen Entwicklungsdruck. Anders als `knight_backrank_penalty`
    /// (flat, beide Grundreihen) bewusst nur die eigene Heimreihe und
    /// phase-getapert, damit im Endspiel ein Laeufer auf der Grundreihe
    /// (z. B. als Verteidiger) nicht bestraft wird. Default 0 = inaktiv.
    pub bishop_backrank_penalty_mg: i32,
    /// **Statischer Anker (16.05.2026, Step 3 ausgerollt):** wird von der
    /// Eval-Logik nicht mehr direkt verwendet — `evaluate_side` taperiert
    /// jetzt ueber `bishop_pair_mg`, `bishop_pair_eg`, `bp_open_scale`
    /// (siehe unten). Feld bleibt fuer Kompatibilitaet alter TOML-Configs
    /// und als Konsistenz mit dem Step-1-Pattern (analog `p.knight` /
    /// `p.bishop`, die ebenfalls als statische Anker stehen).
    pub bishop_pair_each: i32,
    /// Schritt 3 der dynamischen Figurenbewertung (Bishop-Pair, 16.05.2026).
    /// Mittelspiel-Bonus fuer ein vorhandenes Laeuferpaar (Summe fuer das
    /// Paar, NICHT pro Laeufer). Default 30 = altes Verhalten
    /// (`2 * bishop_pair_each` mit `bishop_pair_each = 15`).
    pub bishop_pair_mg: i32,
    /// Endspiel-Basis-Bonus fuer das Laeuferpaar. Im EG profitiert das
    /// Laeuferpaar deutlich (Reichweite, kein Tempoverlust). Default 30,
    /// experimentelle Werte ab 50.
    pub bishop_pair_eg: i32,
    /// Offenheits-Skala: zusaetzliche cp pro fehlendem Bauer auf dem Brett
    /// (Formel: `bishop_pair_eg + (16 - total_pawn_count) * bp_open_scale`,
    /// wirkt nur im EG-Pol des `taper`). Konsistent zur Step-2-Laeufer-
    /// Logik, die `total_pawn_count` als Brett-Offenheits-Maesszahl nutzt.
    /// Default 0 = keine Offenheits-Modulation.
    pub bp_open_scale: i32,
    /// Material-Ungleichgewicht "2 Leichtfiguren vs Turm(+Bauer)" (Diagnose
    /// 31.05.2026: Martuni bewertete Turm+Bauer ≈ Springer+Laeufer und lief so
    /// willentlich in -300-Stellungen, z.B. nach Nxf7 Rxf7 Bxf7+ Qxf7). Bonus
    /// fuer die Minor-Mehrheits-Seite, phase-getapert (MG > EG, da zwei
    /// Leichtfiguren mit Damen am Brett gefaehrlicher sind). Default 0 = inaktiv
    /// (reproduziert altes Verhalten); wirksame Werte in eval.toml `[imbalance]`.
    /// Siehe `eval::material_imbalance`.
    pub imbalance_two_minors_mg: i32,
    pub imbalance_two_minors_eg: i32,
    /// Knight-Outpost-Bonus (Diagnose 05.06.2026): ein durch einen eigenen
    /// Bauern gedeckter Springer auf vorgeschobenem Feld (4.-6. Reihe aus
    /// eigener Sicht), das kein gegnerischer Bauer je angreifen kann, ist
    /// dauerhaft unvertreibbar und positionell wertvoller, als PST/Safe-Mobility
    /// erfassen. Hintergrund: die "motivlosen" Mittelspiel-Drops sind real
    /// (78 % halten bei SF d26) und entstehen durch Figuren-Passivitaet bei
    /// materiellem Gleichstand/Vorteil — Eval ist ~300 cp zu optimistisch.
    /// Phase-getapert (MG > EG: im Mittelspiel zaehlt der raumgewinnende,
    /// blockierende Springer mehr). Default 0 = inaktiv (verhaltensgleich zur
    /// Vorversion); wirksame Werte in eval.toml `[outpost]`.
    /// Siehe `eval::is_knight_outpost`.
    pub outpost_knight_mg: i32,
    pub outpost_knight_eg: i32,
    /// Material-Defizit-Daempfung der "Kompensations"-Terme (Bxe4-Diagnose
    /// 06.06.2026). Befund: Martuni gibt eine Leichtfigur fuer einen Bauern und
    /// bewertet die Folgestellung ~300 cp zu optimistisch, weil ein vorgeschobener
    /// Freibauer (`pawn_bonus`) + gute Figurenfelder (`pst_eg`) den Figurenverlust
    /// kaschieren — obwohl der Gegner eine Mehrfigur zum Blockieren/Schlagen hat.
    /// Wirkung (`eval::material_deficit_damping`): liegt eine Seite statisch um
    /// >= `damp_deficit_threshold` cp zurueck, werden IHR Freibauer-Vormarschbonus
    /// (auf `damp_passed_pct` %) und ihr positiver PST-eg-Ueberschuss (auf
    /// `damp_pst_eg_pct` %) gekuerzt. Defaults: threshold 200 (≈ Leichtfigur fuer
    /// Bauer), beide Prozentsaetze 100 → KEIN Abschlag → verhaltensgleich zur
    /// Vorversion. Wirksame Werte in eval.toml `[damping]`.
    pub damp_deficit_threshold: i32,
    pub damp_passed_pct: i32,
    pub damp_pst_eg_pct: i32,
    pub connected_rooks_pair: i32,
    /// Turm auf vollständig offener Linie (keine eigenen und keine gegnerischen Bauern)
    pub rook_open_file_bonus: i32,
    /// Turm auf halb-offener Linie (keine eigenen, aber gegnerische Bauern)
    pub rook_semiopen_file_bonus: i32,
    /// Tarrasch-Regel: eigener Turm hinter eigenem Freibauer auf derselben Linie.
    /// Klassisches Turmendspiel-Prinzip (Turm schiebt von hinten).
    pub rook_behind_own_passed_bonus: i32,
    /// Gegenstück: eigener Turm hinter gegnerischem Freibauer (Blockade von hinten).
    pub rook_behind_enemy_passed_bonus: i32,
    /// Anti-Bonus: eigener Turm VOR eigenem Freibauer (blockt den eigenen Vormarsch).
    pub rook_blocks_own_passed_penalty: i32,
    /// Turm auf 7. Reihe aus eigener Sicht (Rank 7 weiß, Rank 2 schwarz).
    pub rook_seventh_bonus: i32,
    /// Zusatzbonus, wenn der gegnerische König auf der 8. Reihe (Grundreihe)
    /// steht — dann ist der Turm auf der 7. Reihe eine abschneidende Linie.
    pub rook_seventh_vs_king_eighth_bonus: i32,

    // King safety
    pub ks_knight_weight: i32,
    pub ks_bishop_weight: i32,
    pub ks_rook_weight: i32,
    pub ks_queen_weight: i32,
    pub ks_shield_rank1_bonus: i32,
    pub ks_shield_rank2_bonus: i32,
    pub ks_shield_missing_penalty: i32,
    pub ks_exposed_center_penalty: i32,
    /// Malus fuer den Koenig auf der a- oder f-Linie der Heimreihe —
    /// die Faelle, die `pawn_shield_score` bisher mit 0 durchwinkte
    /// (dorthin kommt der Koenig nur ohne/nach verspielter Rochade).
    /// Negativer Wert, Default 0 = inaktiv. 960-Lookback 26.07.2026:
    /// `exposed_king` war Motiv #2 (53×), Rochadequote nur 46 % vs. 65 %.
    pub ks_uncastled_flank_penalty: i32,
    /// Rochade-Anreiz: Mittelspiel-Malus (`taper(x, 0, phase)`), solange
    /// die Seite noch Rochaderechte hat — rochieren laesst die Rechte und
    /// damit den Malus verschwinden, Koenig-Stehenlassen kostet dauerhaft
    /// Tempo-Aequivalent. Negativer Wert, Default 0 = inaktiv.
    /// Siehe `EngineBoard::has_castle_rights_for` (960-Lookback 26.07.2026).
    pub ks_castle_rights_penalty_mg: i32,
    pub safety_table: Vec<i32>,
    /// Gewichtungsfaktor für die König-Expositions-Strafe (cp pro
    /// Expositions-Punkt). Wirkt nur, wenn der König mindestens 3 Reihen
    /// vom Heimrand entfernt steht UND der Gegner noch nennenswert
    /// Schwergewicht-Material hat (siehe eval::king_exposure_penalty).
    /// Wird zusätzlich phase-getapert: voller Effekt im Mittelspiel,
    /// linear gegen 0 ab `king_activity_phase_threshold`.
    pub king_exposure_weight: i32,

    /// Endspiel-Malus für einen Turm, der direkt von einem eigenen Bauern
    /// blockiert wird (Heimreihe oder zweite Reihe, Bauer eine Reihe vor
    /// dem Turm auf gleicher Linie). Wirkt nur, soweit phase-getapert ins
    /// Endspiel reicht — im vollen Mittelspiel ist 0 (Bauer schützt dort
    /// noch den König oder ist Teil der Eröffnungsstruktur). Negativer Wert.
    pub rook_trapped_endgame_penalty: i32,

    // Endspiel-Mop-up
    pub eg_corner_weight: i32,
    pub eg_king_proximity_weight: i32,
    pub eg_passed_unstoppable_bonus: i32,
    /// Bonus pro Zentralisierungseinheit für den aktiven Endspielkönig.
    /// Skaliert mit (threshold - phase) / threshold, wirkt nur unterhalb threshold.
    pub king_activity_bonus: i32,
    /// Phase-Schwelle (0..24), unterhalb derer König-Aktivität bewertet wird.
    pub king_activity_phase_threshold: i32,
    /// König-Freibauer-Synergie: Bonus cp pro Einheit Chebyshev-Nähe (0..7)
    /// des eigenen Königs zu jedem eigenen Freibauer. Im Endspiel soll der
    /// König den eigenen Freibauer begleiten. Nur unterhalb
    /// `king_activity_phase_threshold` wirksam, skaliert wie king_activity.
    pub king_near_own_passed_bonus: i32,
    /// Spiegelbild: König nah an gegnerischem Freibauer (Blockade-Kandidat).
    /// Etwas stärker gewichtet, weil "Bauer aufhalten" dringlicher ist als
    /// "eigenen Bauer begleiten".
    pub king_near_enemy_passed_bonus: i32,

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
    /// Low-Mobility-Malus fuer Leichtfiguren (960-Lookback 11.08.2026):
    /// Staffel-Malus pro Springer/Laeufer, indiziert mit der Anzahl seiner
    /// SAFE-Zielfelder (Index 0 = voellig vergraben). Zaehlt nur den MG-Pol.
    ///
    /// Motivation: die lineare Safe-Mobility (3 cp/Feld) gibt der Suche kaum
    /// einen Gradienten, eine vergrabene Figur freizuspielen — der Sprung von
    /// 0 auf 2 Felder ist ihr nur 6 cp wert. Genau das sind die "stillen
    /// positionellen Drifts" der 960-Eroeffnungen (35/45 Eroeffnungs-Blunder
    /// ohne Motiv, Eigen-Eval ~67 cp zu optimistisch). Ein konvexer Malus
    /// (z. B. [-30, -20, -10]) macht Entwicklung unmittelbar wertvoll.
    ///
    /// Bewusst NUR Leichtfiguren: Tuerme haben vor der Rochade immer wenig
    /// Mobilitaet (das wuerde nur Rauschen addieren), die Dame soll nicht
    /// zu Fruehausfluegen ermuntert werden.
    ///
    /// Default: leere Liste = Term inaktiv, Eval bit-exakt wie ohne ihn.
    pub minor_low_mob_penalty_mg: Vec<i32>,

    // ----------------------------------------------------------------------
    // Pawn-Endgame-Guard (23.05.2026, Konzept in docs/pawn-endgame-guard.md)
    //
    // Drei Sub-Konzepte fuer einfache Endspiele mit wenig Offizier-Material:
    //   - Opposition (direkt + diagonal): Bonus, wenn der eigene Koenig die
    //     Opposition gegen den gegnerischen haelt (gleiche Linie/Reihe/Diagonale,
    //     eine ungerade Anzahl freier Felder dazwischen, und der GEGNER ist
    //     am Zug).
    //   - Key Squares vor eigenen Freibauern: Bonus pro Freibauer, wenn der
    //     eigene Koenig auf einem der drei Schluesselfelder dieses Bauern
    //     steht. Bonus skaliert mit Bauern-Rang (per Spielerfarbe gespiegelt).
    //   - Rook-Pawn-Edge: Korrektur, wenn der eigene Frei-Bauer auf a- oder
    //     h-Linie steht UND der gegnerische Koenig die Promo-Ecke schon
    //     erreicht oder einen Schritt entfernt ist.
    //
    // Aktivierung: hartes Gate ueber NPM beider Seiten (max. ein Officer-Pool
    // pro Seite ≤ npm_endgame_gate). Im Mittelspiel komplett inaktiv.
    // Zusaetzliches Phase-Tapering analog `king_passed_pawn_synergy`,
    // damit kein Sprung an der Phase-Grenze entsteht.
    //
    // Defaults im Code sind verhaltensgleich (Boni alle 0, Korrektur 0).
    // Erst eine Sektion `[endgame_guard]` in eval.toml aktiviert das Feature.
    /// Bonus (cp), wenn die eigene Seite die Opposition haelt.
    pub opposition_bonus: i32,
    /// Bonus (cp) pro eigenem Freibauer, dessen Schluesselfeld vom eigenen
    /// Koenig besetzt ist. Index = Bauer-Rang 1..8 aus eigener Sicht
    /// (weisser Bauer auf Rang 5 → Index 4, schwarzer Bauer auf Rang 4
    /// → Index 4 nach Spiegelung). Index 0 und 7 sind unmoeglich
    /// (Bauer kann nicht auf Grund-/Umwandlungsreihe stehen).
    pub key_square_bonus_by_rank: [i32; 8],
    /// Strafe (cp), wenn der eigene Frei-Bauer ein Rook-Pawn (a/h) ist und
    /// der gegnerische Koenig die Promo-Ecke kontrolliert. Negativer Wert
    /// — wirkt korrigierend auf den vorhandenen Passbauer-Bonus.
    pub rook_pawn_drawish_penalty: i32,
    /// NPM-Hardgate: der Pawn-Endgame-Guard wird nur ausgewertet, wenn das
    /// Offizier-Material BEIDER Seiten ≤ Gate ist (gemessen ohne Koenig,
    /// nach den statischen Anker-Werten p.knight/bishop/rook/queen).
    /// 700 cp = max. 2 Leichtfiguren oder 1 Turm pro Seite.
    pub npm_endgame_gate: i32,
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

            // Dynmat-Defaults verhaltensgleich (alle 300). Der Phase-Tapering-
            // Codepfad ist somit aktiv, liefert aber bei diesen Defaults exakt
            // den statischen Wert. Wirkliche Werte kommen aus eval.toml.
            knight_mg: 300,
            knight_eg: 300,
            bishop_mg: 300,
            bishop_eg: 300,

            // Step-2-Defaults: ebenfalls 0 → kein Pawn-Adjustment, solange
            // eval.toml keine Werte überschreibt. Damit bleibt das alte
            // Step-1-Verhalten ohne TOML-Override exakt erhalten.
            knight_pawn_scale: 0,
            bishop_pawn_scale: 0,

            pawn_isolated_penalty: -20,
            pawn_de_file_bonus: 10,
            pawn_cf_file_bonus: 5,
            pawn_phalanx_triple: 30,
            pawn_phalanx_double: 15,
            // Rank 0 (Ausgangsreihe) bis Rank 5 (eine Reihe vor Umwandlung).
            // Im Mittelspiel ist ein a2-Freibauer kaum gefährlich; ein a7-Freibauer ist es sehr.
            pawn_passed_rank_bonuses: vec![5, 15, 30, 55, 100, 170],

            knight_backrank_penalty: -50,
            bishop_backrank_penalty_mg: 0,
            // Anker — wird von evaluate_side nicht mehr genutzt (siehe oben),
            // bleibt fuer Kompatibilitaet/Konsistenz.
            bishop_pair_each: 15,
            // Step-3-Defaults verhaltensgleich zum alten `2 * 15 = 30 cp`
            // statisch in beiden Phasen. Echte Werte kommen aus eval.toml
            // (`[material_dynamic]`).
            bishop_pair_mg: 30,
            bishop_pair_eg: 30,
            bp_open_scale: 0,
            imbalance_two_minors_mg: 0,
            imbalance_two_minors_eg: 0,
            // Code-Default 0 → Term inaktiv, Build verhaltensgleich. Rollout-Wert
            // erst nach dem Diagnose-Gate (Optimismus-Gap auf den 35 Stellungen)
            // + A/B + Lichess in eval.toml `[outpost]` eintragen.
            outpost_knight_mg: 0,
            outpost_knight_eg: 0,
            // Defizit-Schwelle 200 cp ≈ Leichtfigur fuer einen Bauern. Beide
            // Prozentsaetze 100 → kein Abschlag → Build verhaltensgleich. Rollout-
            // Werte (z.B. 50/50) erst nach A/B + Lichess in eval.toml `[damping]`.
            damp_deficit_threshold: 200,
            damp_passed_pct: 100,
            damp_pst_eg_pct: 100,
            connected_rooks_pair: 150,
            rook_open_file_bonus: 30,
            rook_semiopen_file_bonus: 15,
            // Tarrasch-Wertebereich: +15 hinten klassisch, +20 hinter gegnerischem
            // (Blockade lähmt Gegner komplett), -10 vor dem eigenen (weniger
            // schlimm, der Bauer bleibt Freibauer).
            rook_behind_own_passed_bonus: 15,
            rook_behind_enemy_passed_bonus: 20,
            rook_blocks_own_passed_penalty: -10,
            // 7. Reihe: +15 Standard, +15 extra wenn König auf Grundreihe
            // eingesperrt (zusammen +30 ≈ einem 7th-Rank-Invasion-Bonus).
            rook_seventh_bonus: 15,
            rook_seventh_vs_king_eighth_bonus: 15,

            ks_knight_weight: 2,
            ks_bishop_weight: 2,
            ks_rook_weight: 3,
            ks_queen_weight: 5,
            ks_shield_rank1_bonus: 10,
            ks_shield_rank2_bonus: 5,
            ks_shield_missing_penalty: -15,
            ks_exposed_center_penalty: -30,
            ks_uncastled_flank_penalty: 0,
            ks_castle_rights_penalty_mg: 0,
            safety_table: DEFAULT_SAFETY_TABLE.to_vec(),
            // Default 12 (von 20 reduziert am 26.04.2026 nach 167-Partien-Auswertung):
            // king_exposure war zu pessimistisch im Endspiel-Übergang, Endgame-
            // Blunder pro Partie waren von 0.33 auf 0.49 gestiegen, Eval-Pessimismus
            // mehrfach 500-1000cp gegenüber Stockfish. Drei Stellschrauben gleichzeitig:
            //   - weight 20 → 12 (Halbierung des Roh-Malus)
            //   - rank_dist >= 3 (statt 2; siehe king_exposure_penalty)
            //   - Phase-Tapering Richtung 0 unterhalb king_activity_phase_threshold
            // Mochi-Beispiel (Kg4, rank_dist=4, enemy_npm=1600cp, phase≈18):
            //   exposure = 3 * 1600 / 1000 = 4 → penalty = 4 * 12 = 48cp
            //   gegen Kg6 (rank_dist=2, jetzt unter Schwelle) → 0cp.
            // Differenz Kg4↔Kg6 stieg sogar von 60cp auf 48cp — ähnlich, aber
            // sauberer abgegrenzt: Kg6 wird gar nicht mehr bestraft.
            king_exposure_weight: 12,
            // -10 cp im vollen Endspiel (phase=0). Im Mittelspiel (phase≥16) 0.
            // Klein gewählt: Tarrasch-rule ist primär ein Übergangs-Hinweis,
            // kein hartes Material-Argument. Ziel: in technischen Endspielen
            // zieht die Engine ihren Turm aktiv aus der Heimreihe.
            rook_trapped_endgame_penalty: -10,

            eg_corner_weight: 20,
            eg_king_proximity_weight: 10,
            eg_passed_unstoppable_bonus: 500,
            king_activity_bonus: 3,
            king_activity_phase_threshold: 16,
            // 0..7 Nähe-Einheiten pro Bauer, vor Phase-Skalierung. Bei Nähe 0
            // (König auf dem Freibauer-Feld) und einem einzelnen eigenen
            // Freibauer: 7 * 2 = 14 cp Raw → im vollen Endspiel (phase=0) 14cp.
            king_near_own_passed_bonus: 2,
            king_near_enemy_passed_bonus: 3,

            knight_mg_mobility: 3,
            knight_eg_mobility: 3,
            bishop_mg_mobility: 3,
            bishop_eg_mobility: 4,
            rook_mg_mobility: 2,
            rook_eg_mobility: 5,
            queen_mg_mobility: 1,
            queen_eg_mobility: 2,
            minor_low_mob_penalty_mg: Vec::new(),

            // Pawn-Endgame-Guard: alle Boni 0 = verhaltensgleich zur alten
            // Eval, solange `[endgame_guard]` in eval.toml fehlt. Das Gate
            // hat einen sinnvollen Default (700 cp), damit es bei einem
            // partiellen Override (z.B. nur opposition_bonus gesetzt) nicht
            // versehentlich auf 0 steht und den Term komplett blockiert.
            opposition_bonus: 0,
            key_square_bonus_by_rank: [0, 0, 0, 0, 0, 0, 0, 0],
            rook_pawn_drawish_penalty: 0,
            npm_endgame_gate: 700,
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

        // [material_dynamic] — optional; Defaults fallen auf den statischen
        // Anker zurück (z.B. knight_mg = p.knight = 300), damit eine fehlende
        // Sektion wirklich neutral ist. So spielen Baseline-Binaries (die
        // diese Sektion noch nicht kennen) und das Step-1-Binary mit derselben
        // eval.toml unterschiedlich nur dann, wenn der Override gesetzt wurde.
        let dyn_mat = section(v, "material_dynamic");
        p.knight_mg = i(&dyn_mat, "knight_mg", p.knight);
        p.knight_eg = i(&dyn_mat, "knight_eg", p.knight);
        p.bishop_mg = i(&dyn_mat, "bishop_mg", p.bishop);
        p.bishop_eg = i(&dyn_mat, "bishop_eg", p.bishop);
        // Step 2 (Pawn-Adjustment): bei fehlender Sektion bleiben die Werte
        // auf 0 (Default in EvalParams), das Feature ist dann neutral.
        p.knight_pawn_scale = i(&dyn_mat, "knight_pawn_scale", p.knight_pawn_scale);
        p.bishop_pawn_scale = i(&dyn_mat, "bishop_pawn_scale", p.bishop_pawn_scale);

        // Step 3 (Bishop-Pair-Tapering): Defaults 30/30/0 = altes Verhalten,
        // beliebige Werte ueberschreiben via [material_dynamic] in eval.toml.
        p.bishop_pair_mg = i(&dyn_mat, "bishop_pair_mg", p.bishop_pair_mg);
        p.bishop_pair_eg = i(&dyn_mat, "bishop_pair_eg", p.bishop_pair_eg);
        p.bp_open_scale = i(&dyn_mat, "bp_open_scale", p.bp_open_scale);

        // [imbalance] — "2 Leichtfiguren vs Turm(+Bauer)"-Bonus (Diagnose 31.05.2026).
        let imb = section(v, "imbalance");
        p.imbalance_two_minors_mg = i(&imb, "two_minors_mg", p.imbalance_two_minors_mg);
        p.imbalance_two_minors_eg = i(&imb, "two_minors_eg", p.imbalance_two_minors_eg);

        // [outpost] — Knight-Outpost-Bonus (Diagnose 05.06.2026). Ohne Sektion
        // bleibt der Code-Default 0 (Term inaktiv, verhaltensgleich).
        let outp = section(v, "outpost");
        p.outpost_knight_mg = i(&outp, "knight_mg", p.outpost_knight_mg);
        p.outpost_knight_eg = i(&outp, "knight_eg", p.outpost_knight_eg);

        // [damping] — Material-Defizit-Daempfung der Kompensations-Terme
        // (Diagnose 06.06.2026, Bxe4-Bug). Ohne Sektion bleiben die Defaults
        // (Prozentsaetze 100 → kein Abschlag, verhaltensgleich).
        let damp = section(v, "damping");
        p.damp_deficit_threshold = i(&damp, "deficit_threshold", p.damp_deficit_threshold);
        p.damp_passed_pct = i(&damp, "passed_pct", p.damp_passed_pct);
        p.damp_pst_eg_pct = i(&damp, "pst_eg_pct", p.damp_pst_eg_pct);

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
        p.bishop_backrank_penalty_mg = i(
            &pc,
            "bishop_backrank_penalty_mg",
            p.bishop_backrank_penalty_mg,
        );
        p.bishop_pair_each = i(&pc, "bishop_pair_each", p.bishop_pair_each);
        p.connected_rooks_pair = i(&pc, "connected_rooks_pair", p.connected_rooks_pair);
        p.rook_open_file_bonus = i(&pc, "rook_open_file_bonus", p.rook_open_file_bonus);
        p.rook_semiopen_file_bonus = i(&pc, "rook_semiopen_file_bonus", p.rook_semiopen_file_bonus);
        p.rook_behind_own_passed_bonus = i(
            &pc,
            "rook_behind_own_passed_bonus",
            p.rook_behind_own_passed_bonus,
        );
        p.rook_behind_enemy_passed_bonus = i(
            &pc,
            "rook_behind_enemy_passed_bonus",
            p.rook_behind_enemy_passed_bonus,
        );
        p.rook_blocks_own_passed_penalty = i(
            &pc,
            "rook_blocks_own_passed_penalty",
            p.rook_blocks_own_passed_penalty,
        );
        p.rook_seventh_bonus = i(&pc, "rook_seventh_bonus", p.rook_seventh_bonus);
        p.rook_seventh_vs_king_eighth_bonus = i(
            &pc,
            "rook_seventh_vs_king_eighth_bonus",
            p.rook_seventh_vs_king_eighth_bonus,
        );
        p.rook_trapped_endgame_penalty = i(
            &pc,
            "rook_trapped_endgame_penalty",
            p.rook_trapped_endgame_penalty,
        );

        let ks = section(v, "king_safety");
        p.ks_knight_weight = i(&ks, "knight_weight", p.ks_knight_weight);
        p.ks_bishop_weight = i(&ks, "bishop_weight", p.ks_bishop_weight);
        p.ks_rook_weight = i(&ks, "rook_weight", p.ks_rook_weight);
        p.ks_queen_weight = i(&ks, "queen_weight", p.ks_queen_weight);
        p.ks_shield_rank1_bonus = i(&ks, "shield_rank1_bonus", p.ks_shield_rank1_bonus);
        p.ks_shield_rank2_bonus = i(&ks, "shield_rank2_bonus", p.ks_shield_rank2_bonus);
        p.ks_shield_missing_penalty = i(&ks, "shield_missing_penalty", p.ks_shield_missing_penalty);
        p.ks_exposed_center_penalty = i(&ks, "exposed_center_penalty", p.ks_exposed_center_penalty);
        p.ks_uncastled_flank_penalty = i(
            &ks,
            "uncastled_flank_penalty",
            p.ks_uncastled_flank_penalty,
        );
        p.ks_castle_rights_penalty_mg = i(
            &ks,
            "castle_rights_penalty_mg",
            p.ks_castle_rights_penalty_mg,
        );
        p.king_exposure_weight = i(&ks, "exposure_weight", p.king_exposure_weight);

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
        p.eg_king_proximity_weight = i(&eg, "king_proximity_weight", p.eg_king_proximity_weight);
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
        p.king_near_own_passed_bonus = i(
            &eg,
            "king_near_own_passed_bonus",
            p.king_near_own_passed_bonus,
        );
        p.king_near_enemy_passed_bonus = i(
            &eg,
            "king_near_enemy_passed_bonus",
            p.king_near_enemy_passed_bonus,
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
        // Staffel-Malus fuer vergrabene Leichtfiguren; fehlender Key oder
        // leere Liste = Term inaktiv (Code-Default bleibt bestehen).
        if let Some(arr) = mob
            .and_then(|s| s.get("minor_low_mob_penalty_mg"))
            .and_then(|v| v.as_array())
        {
            let parsed: Vec<i32> = arr
                .iter()
                .filter_map(|v| v.as_integer().map(|x| x as i32))
                .collect();
            if !parsed.is_empty() {
                p.minor_low_mob_penalty_mg = parsed;
            }
        }

        // [endgame_guard] (23.05.2026, Pawn-Endgame-Guard). Optional; bei
        // fehlender Sektion bleiben die Boni auf 0 (Default in EvalParams),
        // das Feature ist dann neutral.
        let eg_guard = section(v, "endgame_guard");
        p.opposition_bonus = i(&eg_guard, "opposition_bonus", p.opposition_bonus);
        p.rook_pawn_drawish_penalty = i(
            &eg_guard,
            "rook_pawn_drawish_penalty",
            p.rook_pawn_drawish_penalty,
        );
        p.npm_endgame_gate = i(&eg_guard, "npm_endgame_gate", p.npm_endgame_gate);
        if let Some(arr) = eg_guard
            .and_then(|s| s.get("key_square_bonus_by_rank"))
            .and_then(|v| v.as_array())
        {
            let parsed: Vec<i32> = arr
                .iter()
                .filter_map(|v| v.as_integer().map(|x| x as i32))
                .collect();
            // Genau 8 Eintraege erwartet (Index per Bauer-Rang 1..8).
            if parsed.len() == 8 {
                for (i, val) in parsed.into_iter().enumerate() {
                    p.key_square_bonus_by_rank[i] = val;
                }
            } else {
                println!(
                    "info string eval: [endgame_guard].key_square_bonus_by_rank \
                     muss 8 Eintraege haben (gefunden {}), ignoriert",
                    parsed.len()
                );
            }
        }

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
