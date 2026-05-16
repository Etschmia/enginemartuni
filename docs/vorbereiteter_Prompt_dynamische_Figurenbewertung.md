Ich möchte die bisher statische Figurenbewertung (Springer und Läufer jeweils 300 cp) durch eine dynamische Figurenbewertung mit Phasen-Tapering ersetzen.

Wir stehen nicht bei null: Boni für Läuferpaar etc. sind schon implementiert. Aber die starre 300/300-Wertung für Springer und Läufer kann nun zum Thema werden, nachdem die großen Brocken (NMP, PVS, Repetition-Fix, King-Exposure-Tapering) sitzen.

## Theoretischer Hintergrund

Die Mechanik orientiert sich an Kaufman 1999 („The Evaluation of Material Imbalances"):
- Im **Mittelspiel** (MG) ist der Springer wegen taktischer Kooperation mit Bauern marginal stärker oder gleichwertig.
- Im **Endspiel** (EG) gewinnt der Läufer durch Reichweite und Freibauern-Begleitung.
- Die N–B-Differenz **verschiebt sich phasenabhängig** — sie ist im MG nahezu null, im EG klar zugunsten des Läufers.
- Zusätzlich profitiert der Springer von eigenen Bauern auf dem Brett (geschlossene Stellungen), der Läufer leidet darunter (verstellte Diagonalen).

Phase-Skala wie in `pst.rs` bereits etabliert: 24 = Startaufstellung, 0 = leeres Brett.

## Geplante Parameter (Erweiterung von `eval.toml`)

```toml
[material_dynamic]
# Basiswerte für Tapering (statt nur 300/300)
knight_mg = 310
knight_eg = 290
bishop_mg = 305
bishop_eg = 320

# Dynamische Skalierung (Bauern-Adjustment)
knight_pawn_scale = 3   # cp pro eigenem Bauer (Springer profitiert von Bauern)
bishop_pawn_scale = 4   # cp pro fehlendem Bauer auf dem Brett (Läufer profitiert von offenen Linien)

# Läuferpaar-Tuning
bishop_pair_mg = 30     # Bonus im Mittelspiel (gesamt)
bishop_pair_eg = 50     # Basis-Bonus im Endspiel
bp_open_scale  = 2      # Extra-Bonus pro fehlendem Bauer auf dem Brett (gesamt)
```

## Mathematische Logik (Pseudocode)

```
// Hilfsfunktion für lineares Tapering
int interpolate(int mg_val, int eg_val, int phase) {
    // phase 24 = volles MG, phase 0 = volles EG
    return (mg_val * phase + eg_val * (24 - phase)) / 24;
}

int evaluate_material(Board board, EvalConfig config, int phase) {
    int score = 0;
    int total_pawns = board.total_pawn_count();   // 0..16, für Linien-Offenheit
    int us_pawns    = board.pawns_of_color(US).count();  // 0..8

    // --- SPRINGER ---
    // Profitiert von eigenen Bauern (Hilfsstruktur, Outpost-Stützen).
    int n_count = board.knights(US).count();
    int n_pawn_adj = (us_pawns - 8) * config.knight_pawn_scale;
    int n_final_base = interpolate(config.knight_mg, config.knight_eg, phase);
    score += n_count * (n_final_base + n_pawn_adj);

    // --- LÄUFER ---
    // Profitiert von offener Gesamtstellung (weniger Bauern auf dem Brett insgesamt).
    // Hinweis: nutzt total_pawns (16 = volles Brett), nicht nur eigene Bauern —
    // konsistent mit der Läuferpaar-Logik unten.
    int b_count = board.bishops(US).count();
    int b_pawn_adj = (16 - total_pawns) * config.bishop_pawn_scale;
    int b_final_base = interpolate(config.bishop_mg, config.bishop_eg, phase);
    score += b_count * (b_final_base + b_pawn_adj);

    // --- LÄUFERPAAR ---
    if (b_count >= 2) {
        int mg_bp = config.bishop_pair_mg;
        int eg_bp = config.bishop_pair_eg + (16 - total_pawns) * config.bp_open_scale;
        score += interpolate(mg_bp, eg_bp, phase);
    }

    return score;
}
```

Hinweis zur Konsistenz: Im ursprünglichen Entwurf nutzte die Einzelläufer-Logik
`(8 - us_pawns)`, die Läuferpaar-Logik aber `(16 - total_pawns)`. Das war
inkonsistent — „offene Stellung" hängt an der Gesamt-Bauernzahl, nicht nur an
meinen Bauern. Oben jetzt einheitlich `total_pawns` für alle „offene
Diagonalen"-Effekte. Bitte prüfe das nochmal sachlich, ob ich richtig liege.

## Vor der Implementierung zu klären

Die folgenden Punkte will ich erst geklärt haben, bevor wir Code schreiben.
Bitte arbeite sie der Reihe nach durch, **mit Optionen und Empfehlung — keine
einseitige Festlegung.**

### 1. Phase-Definition wiederverwenden

In `pst.rs` ist die Phase-Berechnung schon implementiert (24-Skala). Bestätige,
dass `evaluate_material` diese Funktion **als Parameter entgegennimmt** und keine
eigene parallele Phase-Berechnung einführt. Sonst riskieren wir Sprünge zwischen
PST-Tapering und Material-Tapering. Wenn aus irgendeinem Grund eine eigene
Phase-Definition nötig wäre: warum, und mit welcher Sicherheit dass beide
synchron bleiben.

### 2. Eval-Anker-Frage (mein Hauptbedenken)

Aktuell sind N=B=300 cp **der Anker**, an dem alle anderen Eval-Terme implizit
hängen: King-Safety-SafetyTable, PST-Werte, Outpost-Boni, Pawn-Shield. Wenn ich
auf knight_mg=310 / bishop_eg=320 gehe, verschiebe ich diesen Anker um ~3–7 %.
Damit wirken alle anderen Terme relativ leicht schwächer oder stärker, ohne dass
ich sie angefasst habe.

Konkret: Pawn-Shield-Bonus von z. B. 25 cp ist heute „grob 1/12 Springer wert"
— nach der Änderung im EG nur noch ~8,6 % eines Läuferpaar-Bonus. Das sind
exakt die Hebel, die die 28.04.-Verbesserungen (King-Exposure-Tapering) erst
möglich gemacht haben.

Frage an dich: Müssen King-Safety-Gewichte, Pawn-Shield, Outpost-Boni und
ggf. Mobility-Werte bei dieser Änderung **proportional mitskalieren**, oder
kann man sie konstant lassen? Wenn konstant: warum ist das sicher? Wenn
mitskalieren: zeig mir, welche Terme betroffen sind und wie der Skalierungs-
Faktor pro Phase aussehen sollte. Bitte mit einem konkreten Blick in `eval.rs`
und `eval_config.rs`, nicht abstrakt.

### 3. Kalibrierung der Skalen

Die Werte `knight_pawn_scale = 3` und `bishop_pawn_scale = 4` sind meine
Hausnummern. Kaufman gibt empirisch eher ~5 cp pro Bauer für die
Springer/Bauer-Kopplung an (manchmal als „pawn adjustment 5 cp/pawn"
benannt). Vorschlag: schau dir an, was Stockfish/andere Open-Source-Engines
für vergleichbare Terme nutzen, und bewerte ob meine Werte zu konservativ
sind. Zur Klärung: nicht abschreiben, nur als Referenz für die Größenordnung.

### 4. Mess-Setup, bevor wir Lichess deployen

Bei NMP+PVS und der 28.04.-Änderung haben wir Vorher/Nachher-Auswertungen
gefahren — das war goldrichtig. Hier brauche ich dasselbe.

Liefer mir einen Vorschlag für ein **A/B-Test-Setup**:
- Wahrscheinlich cutechess-cli oder fastchess als Match-Runner (welche
  Variante schon installiert ist, bitte prüfen).
- Mindestens 200 Spiele bei kurzer TC (z. B. 10+0.1 oder 8+0.08), gleicher
  Eröffnungs-Pool, abwechselnde Farben.
- Klar definiertes Erfolgs-Kriterium: Elo-Differenz mit Konfidenzintervall.
- Falls Match negativ ausfällt: Rollback-Strategie ohne Lichess-Auswirkung.

Das Setup will ich **vor der Code-Implementierung** stehen haben, damit klar
ist, woran wir das Ergebnis messen.

## Schrittweiser Rollout (statt Big-Bang)

Der Entwurf führt drei Mechaniken gleichzeitig ein:
- **(a)** MG/EG-Tapering der Basiswerte
- **(b)** Pawn-Scale-Adjustment
- **(c)** Dynamisches Läuferpaar (Phase + Offenheit)

Wenn alle drei in einem Wurf rausgehen und die Auswertung gemischte Signale
zeigt, weiß ich nicht, welcher Effekt zieht. Saubere Variante:

1. **Schritt 1:** Nur (a) einbauen — Tapering der Basiswerte, mit
   `knight_pawn_scale = bishop_pawn_scale = 0` und unverändertem Läuferpaar.
   Mess-Match gegen Baseline. Erwartung: kleiner positiver Effekt im EG.
2. **Schritt 2:** (b) dazu — Pawn-Adjustment aktivieren. Mess-Match gegen
   Stand nach Schritt 1.
3. **Schritt 3:** (c) — Läuferpaar dynamisch. Mess-Match gegen Stand nach
   Schritt 2.

Dauert länger, aber jeder Effekt ist attribuierbar. Analog zur „erst NMP+PVS,
jetzt LMR"-Disziplin.

## Was ich am Ende konkret von dir brauche

1. **Antworten auf die vier Klärungspunkte oben**, mit Optionen und
   Empfehlung — bevor Code geschrieben wird.
2. **Struktur-Update:** Wie sollte die Config-Struktur in Rust aussehen
   (`eval_config.rs` und Loader für `eval.toml`)?
3. **Implementierung:** Erst nach Klärung — eine Rust-Funktion
   `evaluate_material`, die das Board-Objekt nutzt, die Phase entgegennimmt
   und die obige Logik anwendet. Beginnend mit Schritt 1 des Rollouts (nur
   Tapering, kein Pawn-Adjustment, kein dynamisches Pair).
4. **Großzügige Inline-Kommentare** wie bei den anderen Eval-Modulen — das
   ist Lernmaterial für mich, nicht nur Engine-Code.

Wichtig: Der Engine-Logik-Teil (welche Werte, welche Formeln, welche Phase-
Behandlung) soll meine Eigenarbeit bleiben. Du darfst Optionen aufzeigen und
Empfehlungen geben, aber **ich entscheide**. Bei Strukturfragen
(`eval_config.rs`-Layout) und Infrastruktur darfst du direkt vorschlagen.


## Meine Antwort Stand 10.05.2026

Einverstanden, wir machen keinen Big-Bang.

1. Phase
Wir verwenden ausschließlich die vorhandene Phase aus eval.rs: `game_phase(board)` und `taper(...)`.
Kleine Korrektur: Die Phase-Berechnung liegt aktuell nicht in pst.rs, sondern in eval.rs. `evaluate()` berechnet die Phase bereits einmal und gibt sie an `evaluate_side(...)` weiter. Für dynamisches Material soll keine zweite Phase-Berechnung entstehen.

2. Eval-Anker
Ich will in Schritt 1 keine proportionale Mitskalierung von King-Safety, Pawn-Shield, Mobility usw.

Begründung:
- Die Centipawn-Skala bleibt über `pawn = 100` stabil.
- Die Änderung soll nur Material-Imbalances N vs B anders bewerten.
- Eine automatische Mitskalierung aller Positionswerte wäre selbst eine zweite Eval-Änderung und würde den A/B-Test unklar machen.
- Wichtig: `p.knight` und `p.bishop` bleiben als statische Referenzwerte für bestehende Logik erhalten, insbesondere `king_exposure_penalty` und Endgame-Materialzählungen. Neue dynamische Werte werden nur für den eigentlichen Materialscore verwendet.
- Outpost-Boni gibt es im aktuellen Code offenbar noch nicht; also dort nichts mitskalieren.

3. Rollout
Wir starten nur mit Schritt 1:
- dynamische Felder für `knight_mg`, `knight_eg`, `bishop_mg`, `bishop_eg`
- `knight_pawn_scale = 0`
- `bishop_pawn_scale = 0`
- Läuferpaar bleibt unverändert `2 * bishop_pair_each`

Pawn-Adjustment und dynamisches Läuferpaar kommen erst nach separatem positivem oder zumindest unauffälligem Test.

4. Config-Struktur
Bitte `[material]` als statischen Anker behalten:
pawn=100, knight=300, bishop=300, rook=500, queen=900.

Neue Werte separat, z. B. `[material_dynamic]`.
Defaults sollen verhaltensgleich sein:
knight_mg=300, knight_eg=300, bishop_mg=300, bishop_eg=300, scales=0.
Damit ist das Feature ohne TOML-Änderung neutral.

5. Kalibrierung
Kaufman/Stockfish/andere Engines nur als Größenordnung ansehen, nicht als Vorlage.
Da Martuni bereits Bishop-EG-Mobility und PST-Tapering hat, vorsichtig starten. Die vorgeschlagenen Werte sind als Experiment okay, aber nicht gleichzeitig mit Pawn-Scaling testen.

6. Mess-Setup
Vor Code bitte Runner klären. Auf meinem System sind weder `cutechess-cli` noch `fastchess` im PATH.
Ich bevorzuge cutechess-cli oder fastchess mit:
- zwei Release-Binaries: Baseline und dynmat_step1
- gleiche Hash-Größe, Ponder aus, gleiche TC
- feste Opening-Suite, Farben gespiegelt
- mindestens 200 Spiele als Smoke-Test, besser 400+
- Erfolg nur bei positiver Elo-Tendenz ohne klare Regressionssignale
- Rollback: Lichess-Binary bleibt Baseline, bis der Test bestanden ist

Danach erst Implementierung von Schritt 1.

Der wichtigste Punkt ist: p.knight/p.bishop nicht global ersetzen, sondern neue dynamische Materialfelder ergänzen. Sonst berühren wir indirekt auch king_exposure_penalty (line 234) und Endgame-Materialzählungen.