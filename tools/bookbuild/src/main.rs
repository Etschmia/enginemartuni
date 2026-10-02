//! bookbuild — baut Martunis Eigenbuch für eine Variante als Polyglot-Datei.
//!
//! Eingabe:  book_work/<variante>_book.txt (erzeugt von tools/book_weights.py)
//!           eine Zeile pro Buchzug:  <Zugfolge ab Grundstellung> | <Buchzug> <Gewicht>
//!           "#"-Zeilen sind Kommentare, die Grundstellung hat eine leere Zugfolge.
//! Ausgabe:  Polyglot-Layout, 16 Byte pro Eintrag, Big-Endian, nach Schlüssel sortiert:
//!           key: u64 | move: u16 | weight: u16 | learn: u32 (immer 0)
//!
//! Der Schlüssel kommt aus shakmaty (`zobrist_hash::<Zobrist64>(EnPassantMode::Legal)`).
//! Für KotH ist das exakt der Polyglot-Schlüssel (KotH hat keinen Zusatzzustand).
//! Für Three-check mischt shakmaty die verbleibenden Schachs mit ein, sobald eine
//! Seite weniger als 3 übrig hat. Damit unterscheidet das Buch "gleiche Figuren,
//! aber schon ein Schach gegeben" von der Stellung ohne Schach. python-chess
//! kann solche Three-check-Schlüssel deshalb nicht nachrechnen, die Engine
//! (gleiche shakmaty-Version) schon.
//!
//! Aufruf:  bookbuild <3check|koth> <eingabe.txt> <ausgabe.bin>

use std::collections::BTreeMap;
use std::fs;
use std::process::ExitCode;

use shakmaty::uci::UciMove;
use shakmaty::variant::{KingOfTheHill, ThreeCheck};
use shakmaty::zobrist::{Zobrist64, ZobristHash};
use shakmaty::{EnPassantMode, Move, Position, Role, Square};

/// Ein fertiger Bucheintrag, genau wie er später in der Datei steht.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Entry {
    key: u64,
    mv: u16,
    weight: u16,
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 4 {
        eprintln!("Aufruf: bookbuild <3check|koth> <eingabe.txt> <ausgabe.bin>");
        return ExitCode::from(2);
    }
    let (variant, input, output) = (&args[1], &args[2], &args[3]);

    let text = match fs::read_to_string(input) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("FEHLER: {input} nicht lesbar: {e}");
            return ExitCode::FAILURE;
        }
    };

    // Die Variante bestimmt nur die Startstellung. Alles andere ist generisch
    // über das Position-Trait von shakmaty, deshalb gibt es build() nur einmal.
    let entries = match variant.as_str() {
        "3check" => build::<ThreeCheck>(&text),
        "koth" => build::<KingOfTheHill>(&text),
        _ => Err(format!("unbekannte Variante '{variant}' (erlaubt: 3check, koth)")),
    };
    let entries = match entries {
        Ok(e) => e,
        Err(msg) => {
            eprintln!("FEHLER: {msg}");
            return ExitCode::FAILURE;
        }
    };

    if let Err(e) = fs::write(output, encode(&entries)) {
        eprintln!("FEHLER: {output} nicht schreibbar: {e}");
        return ExitCode::FAILURE;
    }

    // Kontrolle: Datei zurücklesen und mit dem vergleichen, was wir schreiben wollten.
    let back = match fs::read(output).map(|d| decode(&d)) {
        Ok(Ok(b)) => b,
        Ok(Err(e)) | Err(e) => {
            eprintln!("FEHLER: Rücklesen von {output} fehlgeschlagen: {e}");
            return ExitCode::FAILURE;
        }
    };
    if back != entries {
        eprintln!("FEHLER: zurückgelesene Datei weicht vom Soll ab");
        return ExitCode::FAILURE;
    }

    let positions = entries.iter().map(|e| e.key).collect::<std::collections::BTreeSet<_>>().len();
    println!(
        "{variant}: {} Einträge, {positions} Stellungen -> {output} ({} Byte)",
        entries.len(),
        entries.len() * 16
    );
    ExitCode::SUCCESS
}

/// Liest die Textdatei und erzeugt die sortierten Bucheinträge.
///
/// `P::default()` ist die Grundstellung der Variante. Jede Zeile wird von dort
/// aus nachgespielt. So zählt shakmaty die Schachs in Three-check selbst mit,
/// und wir müssen kein FEN mit Schachzählern parsen.
fn build<P: Position + Default + Clone + ZobristHash>(text: &str) -> Result<Vec<Entry>, String> {
    // Schlüssel -> (Zug -> Gewicht). Die Map erkennt doppelte Einträge:
    // Kommt dieselbe Stellung über zwei Zugfolgen (Zugumstellung) zweimal vor,
    // dürfen die Gewichte nicht verschieden sein, sonst wäre das Buch mehrdeutig.
    let mut book: BTreeMap<u64, BTreeMap<u16, u16>> = BTreeMap::new();

    for (lineno, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let where_ = format!("Zeile {}: '{line}'", lineno + 1);

        let (seq, rest) = line
            .split_once('|')
            .ok_or_else(|| format!("{where_}: kein '|' gefunden"))?;

        // 1. Stellung aufbauen: Zugfolge ab Grundstellung nachspielen.
        let mut pos = P::default();
        for uci in seq.split_whitespace() {
            let m = parse_legal(&pos, uci).map_err(|e| format!("{where_}: Zugfolge: {e}"))?;
            pos.play_unchecked(m); // Legalität hat parse_legal schon geprüft
        }

        // 2. Schlüssel.
        let key = pos.zobrist_hash::<Zobrist64>(EnPassantMode::Legal).0;

        // 3. Buchzug und Gewicht lesen und prüfen.
        let mut parts = rest.split_whitespace();
        let (Some(uci), Some(w), None) = (parts.next(), parts.next(), parts.next()) else {
            return Err(format!("{where_}: rechts vom '|' muss '<zug> <gewicht>' stehen"));
        };
        let m = parse_legal(&pos, uci).map_err(|e| format!("{where_}: Buchzug: {e}"))?;
        let weight: u16 = w
            .parse()
            .map_err(|_| format!("{where_}: Gewicht '{w}' ist keine Zahl 0..65535"))?;
        if weight == 0 {
            return Err(format!("{where_}: Gewicht 0 (der Zug würde nie gespielt)"));
        }

        // 4. Zug kodieren und 5. merken.
        let mv = encode_move(&m);
        match book.entry(key).or_default().insert(mv, weight) {
            Some(old) if old != weight => {
                return Err(format!(
                    "{where_}: Stellung schon mit Gewicht {old} für {uci} im Buch (Zugumstellung?)"
                ));
            }
            _ => {}
        }
    }

    if book.is_empty() {
        return Err("keine Buchzüge in der Eingabe".into());
    }

    // 6. Sortieren: Schlüssel aufsteigend (BTreeMap liefert das schon),
    //    innerhalb eines Schlüssels Gewicht absteigend. Der Engine-Loader sucht
    //    per binary_search nach dem Schlüssel, die Reihenfolge innerhalb ist
    //    für ihn egal. Polyglot-Konvention ist aber "bester Zug zuerst".
    let mut entries = Vec::new();
    for (key, moves) in book {
        let mut ms: Vec<(u16, u16)> = moves.into_iter().collect();
        ms.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        entries.extend(ms.into_iter().map(|(mv, weight)| Entry { key, mv, weight }));
    }
    Ok(entries)
}

/// UCI-Text -> legaler shakmaty-Zug in `pos`.
/// `to_move` versteht Standard-Rochade (e1g1) und prüft die Legalität.
fn parse_legal<P: Position>(pos: &P, uci: &str) -> Result<Move, String> {
    let u: UciMove = uci.parse().map_err(|_| format!("'{uci}' ist kein UCI-Zug"))?;
    u.to_move(pos).map_err(|_| format!("'{uci}' ist hier nicht legal"))
}

/// Zug -> 16-Bit-Polyglot-Kodierung (passend zu decode_move in src/polyglot/book.rs):
///
/// ```text
/// Bit  0– 2  Ziel-Linie   (a=0 … h=7)
/// Bit  3– 5  Ziel-Reihe   (1=0 … 8=7)
/// Bit  6– 8  Start-Linie
/// Bit  9–11  Start-Reihe
/// Bit 12–14  Umwandlung   (0 keine, 1 Springer, 2 Läufer, 3 Turm, 4 Dame)
/// ```
///
/// Rochade: Polyglot speichert sie als "König schlägt eigenen Turm" (e1h1 statt
/// e1g1). shakmatys `Move::Castle { king, rook }` bringt das Turmfeld direkt mit.
fn encode_move(m: &Move) -> u16 {
    let (from, to): (Square, Square) = match *m {
        Move::Castle { king, rook } => (king, rook),
        _ => (m.from().expect("Einsetzzüge gibt es in 3check/KotH nicht"), m.to()),
    };
    let promo: u16 = match m.promotion() {
        None => 0,
        Some(Role::Knight) => 1,
        Some(Role::Bishop) => 2,
        Some(Role::Rook) => 3,
        Some(Role::Queen) => 4,
        Some(r) => panic!("unmögliche Umwandlung in {r:?}"),
    };
    let sq = |s: Square| ((s.rank().to_u32() as u16) << 3) | s.file().to_u32() as u16;
    (promo << 12) | (sq(from) << 6) | sq(to)
}

/// Einträge -> Bytes (16 Byte pro Eintrag, Big-Endian, learn = 0).
fn encode(entries: &[Entry]) -> Vec<u8> {
    let mut out = Vec::with_capacity(entries.len() * 16);
    for e in entries {
        out.extend_from_slice(&e.key.to_be_bytes());
        out.extend_from_slice(&e.mv.to_be_bytes());
        out.extend_from_slice(&e.weight.to_be_bytes());
        out.extend_from_slice(&0u32.to_be_bytes());
    }
    out
}

/// Bytes -> Einträge. Gegenstück zu `encode`, gleiche Logik wie Book::load der Engine.
fn decode(data: &[u8]) -> Result<Vec<Entry>, std::io::Error> {
    if data.len() % 16 != 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Dateigröße kein Vielfaches von 16",
        ));
    }
    Ok(data
        .chunks_exact(16)
        .map(|c| Entry {
            key: u64::from_be_bytes(c[0..8].try_into().unwrap()),
            mv: u16::from_be_bytes(c[8..10].try_into().unwrap()),
            weight: u16::from_be_bytes(c[10..12].try_into().unwrap()),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key_after<P: Position + Default + ZobristHash>(moves: &str) -> u64 {
        let mut pos = P::default();
        for uci in moves.split_whitespace() {
            let m = parse_legal(&pos, uci).unwrap();
            pos.play_unchecked(m);
        }
        pos.zobrist_hash::<Zobrist64>(EnPassantMode::Legal).0
    }

    /// Test 1: offizieller Polyglot-Schlüssel der Grundstellung.
    /// Gilt auch für Three-check und KotH (gleiche Grundstellung, 3+3 mischt nichts ein).
    #[test]
    fn grundstellung_hat_polyglot_schluessel() {
        assert_eq!(key_after::<ThreeCheck>(""), 0x463b96181691fc9c);
        assert_eq!(key_after::<KingOfTheHill>(""), 0x463b96181691fc9c);
    }

    /// Bekannter Polyglot-Wert nach 1.e4 (aus der Polyglot-Spezifikation).
    /// Prüft u. a., dass das e3-Feld NICHT als en passant zählt (kein Schläger da).
    #[test]
    fn nach_e4_polyglot_schluessel() {
        assert_eq!(key_after::<KingOfTheHill>("e2e4"), 0x823c9b50fd114196);
    }

    /// Test 2: Three-check unterscheidet die Schachzähler.
    /// Dieselbe Zugfolge in KotH (kein Zähler) und Three-check ergibt dieselbe
    /// Figurenstellung. Ohne Schach müssen die Schlüssel gleich sein (3+3 mischt
    /// nichts ein), nach einem Schach (1.e4 f6 2.Qh5+) verschieden.
    #[test]
    fn three_check_zaehlt_schachs_mit() {
        let ohne_schach = "e2e4 e7e5 g1f3";
        assert_eq!(key_after::<ThreeCheck>(ohne_schach), key_after::<KingOfTheHill>(ohne_schach));

        let mit_schach = "e2e4 f7f6 d1h5";
        assert_ne!(key_after::<ThreeCheck>(mit_schach), key_after::<KingOfTheHill>(mit_schach));
    }

    /// Test 3: Rochade wird als König-schlägt-Turm kodiert.
    #[test]
    fn rochade_als_koenig_auf_turm() {
        let mut pos = ThreeCheck::default();
        for uci in "e2e4 e7e5 g1f3 b8c6 f1c4 g8f6".split_whitespace() {
            let m = parse_legal(&pos, uci).unwrap();
            pos.play_unchecked(m);
        }
        let castle = parse_legal(&pos, "e1g1").unwrap();
        // e1 = Linie 4, Reihe 0; h1 = Linie 7, Reihe 0
        assert_eq!(encode_move(&castle), (4 << 6) | 7);
    }

    #[test]
    fn umwandlung_und_normaler_zug() {
        let e2e4 = Move::Normal {
            role: Role::Pawn,
            from: Square::E2,
            capture: None,
            to: Square::E4,
            promotion: None,
        };
        assert_eq!(encode_move(&e2e4), (1 << 9) | (4 << 6) | (3 << 3) | 4);

        let a7a8q = Move::Normal {
            role: Role::Pawn,
            from: Square::A7,
            capture: None,
            to: Square::A8,
            promotion: Some(Role::Queen),
        };
        assert_eq!(encode_move(&a7a8q), (4 << 12) | (6 << 9) | (7 << 3));
    }

    #[test]
    fn build_sortiert_und_kodiert_rueckwaerts() {
        let text = "# Kommentar\n | g1f3 300\n | e2e4 700\ne2e4 | e7e5 1000\n";
        let entries = build::<KingOfTheHill>(text).unwrap();
        assert_eq!(entries.len(), 3);
        assert!(entries.windows(2).all(|w| w[0].key <= w[1].key));
        let start: Vec<_> = entries.iter().filter(|e| e.key == 0x463b96181691fc9c).collect();
        assert_eq!(start.len(), 2);
        assert_eq!(start[0].weight, 700, "höheres Gewicht zuerst");
        assert_eq!(decode(&encode(&entries)).unwrap(), entries);
    }

    #[test]
    fn build_meldet_illegale_zuege() {
        assert!(build::<KingOfTheHill>(" | e2e5 100\n").is_err());
        assert!(build::<KingOfTheHill>("e2e4 e2e4 | e7e5 100\n").is_err());
        assert!(build::<KingOfTheHill>(" | e2e4 0\n").is_err());
    }
}
