// pst.rs — Piece-Square Tables fuer Martuni.
// Werte in Centipawns, zusaetzlich zum Materialwert.
//
// Tabellen-Layout: **visuell aus Weiss' Sicht**. Die erste Zeile im
// Quellcode entspricht Reihe 8 (oben), die letzte Zeile Reihe 1 (unten).
// Die natuerliche Quadrat-Nummerierung der chess-Crate ist aber A1=0,
// H8=63 — also von unten nach oben.
//
// Fuer den Lookup braucht es deshalb eine Umrechnung: `pst_index(sq, us)`
// liefert fuer ein natuerliches Quadrat den passenden Index in die
// visuelle Tabelle. Wichtig und etwas kontraintuitiv:
//
//   Weiss:   pst_index(sq, White) = sq XOR 56
//   Schwarz: pst_index(sq, Black) = sq
//
// Die Spiegelung laeuft also FUER WEISS — nicht, wie man zunaechst
// erwartet, fuer Schwarz. Grund: die visuelle Tabelle nummeriert von
// Reihe 8 abwaerts, natuerliche Indizes aber von Reihe 1 aufwaerts.
// Fuer Schwarz fallen die beiden Spiegelungen zufaellig zusammen und
// kompensieren sich zu einer Identitaet.

use chess::Color;

pub struct PstSet {
    pub mg: [i32; 64],
    pub eg: [i32; 64],
}

#[inline(always)]
pub fn pst_index(sq: usize, us: Color) -> usize {
    match us {
        Color::White => sq ^ 56,
        Color::Black => sq,
    }
}

// =============================================================================
// PAWNS (Bauern)
// MG: Zentrumskontrolle. EG: Bauernumwandlung forcieren.
// =============================================================================
pub const PAWN_PST: PstSet = PstSet {
    mg: [
         0,  0,  0,  0,  0,  0,  0,  0, // Reihe 8 (nie erreicht)
        50, 50, 50, 50, 50, 50, 50, 50, // Reihe 7
        10, 10, 20, 30, 30, 20, 10, 10, // Reihe 6
         5,  5, 10, 25, 25, 10,  5,  5, // Reihe 5
         0,  0,  0, 20, 20,  0,  0,  0, // Reihe 4
         5, -5,-10,  0,  0,-10, -5,  5, // Reihe 3
         5, 10, 10,-20,-20, 10, 10,  5, // Reihe 2
         0,  0,  0,  0,  0,  0,  0,  0  // Reihe 1
    ],
    eg: [
         0,  0,  0,  0,  0,  0,  0,  0, // Reihe 8
        80, 80, 80, 80, 80, 80, 80, 80, // Reihe 7
        50, 50, 50, 50, 50, 50, 50, 50, // Reihe 6
        30, 30, 30, 30, 30, 30, 30, 30, // Reihe 5
        20, 20, 20, 20, 20, 20, 20, 20, // Reihe 4
        10, 10, 10, 10, 10, 10, 10, 10, // Reihe 3
        10, 10, 10, 10, 10, 10, 10, 10, // Reihe 2
         0,  0,  0,  0,  0,  0,  0,  0  // Reihe 1
    ],
};

// =============================================================================
// KNIGHTS (Springer)
// Lieben das Zentrum (Oktogonaler Radius), hassen den Rand.
// MG/EG Unterscheidung geringer als bei anderen Figuren.
// =============================================================================
pub const KNIGHT_PST: PstSet = PstSet {
    mg: [
        -50,-40,-30,-30,-30,-30,-40,-50,
        -40,-20,  0,  0,  0,  0,-20,-40,
        -30,  0, 10, 15, 15, 10,  0,-30,
        -30,  5, 15, 20, 20, 15,  5,-30,
        -30,  0, 15, 20, 20, 15,  0,-30,
        -30,  5, 10, 15, 15, 10,  5,-30,
        -40,-20,  0,  5,  5,  0,-20,-40,
        -50,-40,-30,-30,-30,-30,-40,-50
    ],
    eg: [
        -50,-40,-30,-30,-30,-30,-40,-50,
        -40,-20,  0,  5,  5,  0,-20,-40,
        -30,  5, 10, 15, 15, 10,  5,-30,
        -30, 10, 20, 25, 25, 20, 10,-30,
        -30, 10, 20, 25, 25, 20, 10,-30,
        -30,  5, 10, 15, 15, 10,  5,-30,
        -40,-20,  0,  5,  5,  0,-20,-40,
        -50,-40,-30,-30,-30,-30,-40,-50
    ],
};

// =============================================================================
// BISHOPS (Läufer)
// Ähnlich Springer, Fokus auf Long-Range Mobilität aus dem Zentrum.
// =============================================================================
pub const BISHOP_PST: PstSet = PstSet {
    mg: [
        -20,-10,-10,-10,-10,-10,-10,-20,
        -10,  0,  0,  0,  0,  0,  0,-10,
        -10,  0,  5, 10, 10,  5,  0,-10,
        -10,  5,  5, 10, 10,  5,  5,-10,
        -10,  0, 10, 10, 10, 10,  0,-10,
        -10, 10, 10, 10, 10, 10, 10,-10,
        -10,  5,  0,  0,  0,  0,  5,-10,
        -20,-10,-10,-10,-10,-10,-10,-20
    ],
    eg: [
        -20,-10,-10,-10,-10,-10,-10,-20,
        -10,  0,  0,  0,  0,  0,  0,-10,
        -10,  0,  5, 10, 10,  5,  0,-10,
        -10,  5, 10, 10, 10, 10,  5,-10,
        -10,  5, 10, 10, 10, 10,  5,-10,
        -10,  0,  5, 10, 10,  5,  0,-10,
        -10,  0,  0,  0,  0,  0,  0,-10,
        -20,-10,-10,-10,-10,-10,-10,-20
    ],
};

// =============================================================================
// ROOKS (Türme)
// MG: Fokus auf 7. Reihe (Angriff) und Zentrum. EG: Mobilität.
// Hinweis: Bonus für offene Linien wird oft separat berechnet.
// =============================================================================
pub const ROOK_PST: PstSet = PstSet {
    mg: [
         0,  0,  0,  5,  5,  0,  0,  0,
        -5,  0,  0,  0,  0,  0,  0, -5,
        -5,  0,  0,  0,  0,  0,  0, -5,
        -5,  0,  0,  0,  0,  0,  0, -5,
        -5,  0,  0,  0,  0,  0,  0, -5,
        -5,  0,  0,  0,  0,  0,  0, -5,
         5, 10, 10, 10, 10, 10, 10,  5, // Nahe 7. Reihe (Bonus)
         0,  0,  0,  0,  0,  0,  0,  0
    ],
    eg: [
        -20,-10,-10, -5, -5,-10,-10,-20,
        -10,  0,  0,  0,  0,  0,  0,-10,
        -10,  0,  5, 10, 10,  5,  0,-10,
         -5,  0, 10, 15, 15, 10,  0, -5,
         -5,  0, 10, 15, 15, 10,  0, -5,
        -10,  0,  5, 10, 10,  5,  0,-10,
        -10,  0,  0,  0,  0,  0,  0,-10,
        -20,-10,-10, -5, -5,-10,-10,-20
    ],
};

// =============================================================================
// QUEEN (Dame)
// Kombiniert Turm- und Läufer-Eigenschaften. Sehr zentrums-affin.
// MG: Eher passiv im Zentrum. EG: Aggressiv und zentralisiert.
// =============================================================================
pub const QUEEN_PST: PstSet = PstSet {
    mg: [
        -20,-10,-10, -5, -5,-10,-10,-20,
        -10,  0,  0,  0,  0,  0,  0,-10,
        -10,  0,  5,  5,  5,  5,  0,-10,
         -5,  0,  5,  5,  5,  5,  0, -5,
          0,  0,  5,  5,  5,  5,  0, -5,
        -10,  5,  5,  5,  5,  5,  0,-10,
        -10,  0,  5,  0,  0,  0,  0,-10,
        -20,-10,-10, -5, -5,-10,-10,-20
    ],
    eg: [
        -50,-40,-30,-20,-20,-30,-40,-50,
        -40,-20,-10,  0,  0,-10,-20,-40,
        -30,-10, 20, 30, 30, 20,-10,-30,
        -20,  0, 30, 40, 40, 30,  0,-20,
        -20,  0, 30, 40, 40, 30,  0,-20,
        -30,-10, 20, 30, 30, 20,-10,-30,
        -40,-20,-10,  0,  0,-10,-20,-40,
        -50,-40,-30,-20,-20,-30,-40,-50
    ],
};

// =============================================================================
// KING (König)
// MG: Sicherheit in der Ecke (hinter Bauernschild). EG: Aktive Zentralisierung.
// =============================================================================
pub const KING_PST: PstSet = PstSet {
    mg: [
        -30,-40,-40,-50,-50,-40,-40,-30,
        -30,-40,-40,-50,-50,-40,-40,-30,
        -30,-40,-40,-50,-50,-40,-40,-30,
        -30,-40,-40,-50,-50,-40,-40,-30,
        -20,-30,-30,-40,-40,-30,-30,-20,
        -10,-20,-20,-20,-20,-20,-20,-10,
         20, 20,  0,  0,  0,  0, 20, 20, // Startposition
         20, 30, 10,  0,  0, 10, 30, 20  // Rochade-Ecken bevorzugt
    ],
    eg: [
        -50,-40,-30,-20,-20,-30,-40,-50,
        -30,-20,-10,  0,  0,-10,-20,-30,
        -30,-10, 20, 30, 30, 20,-10,-30,
        -30,-10, 30, 40, 40, 30,-10,-30,
        -30,-10, 30, 40, 40, 30,-10,-30,
        -30,-10, 20, 30, 30, 20,-10,-30,
        -30,-30,  0,  0,  0,  0,-30,-30,
        -50,-30,-30,-30,-30,-30,-30,-50
    ],
};