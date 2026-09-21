# Betrieb: Martuni-Schleuse Blunder-Analyse — Host SYR-PE-BUTDEV

Dieser Host ist seit 21.09.2026 das Analyse-Backend für die Schleuse
`martuni.de:/home/librechat/grok_bot_schleuse`. Er löst den alten Grok-Bot-Host
ab. Verbindliches Protokoll ist `PROTOKOLL.md` **in der Schleuse**, nicht diese
Datei — hier steht nur, wie es auf diesem Rechner aufgesetzt ist.

Der Bot „Martuni" selbst läuft unverändert auf dem Martuni-Server. Dieser Host
ist ausschließlich Rechenknecht.

## 1. Layout

```
/var/www/but2/botdir/
├── enginemartuni/                    # Repo Etschmia/enginemartuni
│   ├── .venv/                        # Python 3.14, python-chess 1.11.2
│   ├── tools/schleuse_worker.py      # Worker (Pfade per SCHLEUSE_*-Env)
│   ├── tools/schleuse_worker.pid     # PID-Lock (gitignored)
│   └── logs/schleuse_worker.log      # Worker-Log (gitignored)
└── schleuse-work/
    ├── worker.env                    # Host-Konfiguration (keine Geheimnisse)
    ├── analyze_blunders.py           # Referenzkopie AUS DER SCHLEUSE
    └── jobs/                         # Arbeitsverzeichnis: PGN, JSON, status.json
```

**Wichtig zur Referenzkopie:** `schleuse-work/analyze_blunders.py` ist eine
Kopie von `grok_bot_schleuse/analyze_blunders.py`, nicht aus dem Repo. Laut
Protokoll schreibt der Martuni-Server diese Datei, wir lesen sie nur. Aktuell
sind Repo-Stand und Schleusen-Kopie identisch (sha256 geprüft). Wenn der
Martuni-Server das Skript aktualisiert, hier neu holen:

```bash
scp martuni.de:/home/librechat/grok_bot_schleuse/analyze_blunders.py \
    /var/www/but2/botdir/schleuse-work/analyze_blunders.py
sudo systemctl restart martuni-schleuse-worker.service
```

## 2. SSH

Der Worker ruft `ssh martuni.de` **ohne** Benutzernamen auf. Dafür gibt es
einen Alias in `~/.ssh/config`:

```
Host martuni.de
    User librechat
    BatchMode yes
    ConnectTimeout 30
    ServerAliveInterval 30
    ServerAliveCountMax 3
```

Test: `ssh martuni.de 'whoami'` muss `librechat` liefern, ohne Passwortfrage.

## 3. Engines

| Rolle | Binary | Version |
|---|---|---|
| Standard, Chess960, From Position | `/usr/games/stockfish` | **Stockfish 17.1** |
| echte Varianten | `/usr/games/fairy-stockfish` | **Fairy-Stockfish 14.0.1 XQ** |

Beide aus apt (`stockfish 17-1`, `fairy-stockfish 14.0.1.xq-0.4`). Stockfish
ist identisch zum Referenzstand.

### Fairy-Version: bewusste Abweichung

Der alte Grok-Host rechnete Varianten mit **Fairy-Stockfish 11.1 LB 64**, der
Martuni-Server davor mit **Fairy-Stockfish 14** (Release `fairy_sf_14`, Build
`x86-64-bmi2`, manuell unter `~/tools`). Hier läuft das apt-Paket 14.0.1 in
einem **XiangQi-Build** — also weder bit-identisch zu 11.1 noch zu eurem 14.

Von Tobias am 21.09.2026 so entschieden. Tragend war nicht „14 ist die
Baseline", sondern die Bewertung, die das Projekt bereits am 14.09. getroffen
hat, als die Analyse zum Grok-Host wanderte (`PROTOKOLL.md`): Der
Versionsunterschied gilt „für die Blunder/Partie-Statistik unerheblich",
kritisch ist die **Tiefe**. Solange `--depth 17` und `--hash 256` stehen, ist
die Reihe nach dem Maßstab dieser Doku vergleichbar.

**Bruchstelle für spätere Auswertungen** — die Zustandsdatei
`analyse-*-varianten.json` speichert die Engine-Version **nicht** (Keys nur
`version`, `updated_at`, `analyzed_pgns`, `blunders`; kein Engine-Feld je
Eintrag). Ein Versionswechsel ist darin unsichtbar. Deshalb hier festgehalten:

| Zeitraum | Varianten-Engine | Rechner |
|---|---|---|
| bis 13.09.2026 | Fairy-Stockfish 14 (bmi2) | Martuni-Server |
| 14.09.–20.09.2026 | Fairy-Stockfish 11.1 LB 64 | Grok-Bot-Host |
| ab 21.09.2026, ~16:52 CEST | Fairy-Stockfish 14.0.1 XQ | dieser Host |

Nachweisbar über `variant_engine` in `status.json` und die Dateidaten in
`grok_bot_schleuse/done/`.

## 4. Analyseparameter

Unverändert gegenüber dem Referenzstand — **nicht ohne Absprache ändern**,
insbesondere nicht die Tiefe:

| Parameter | Wert |
|---|---|
| `--depth` | 17 |
| `--threads` | 2 |
| `--hash` | 256 |
| `--min-movetime` | 0 |
| Timeout je Partie | 45 min hart |
| Idle-Sleep | 300 s |
| Parallelität | 1 Partie, älteste zuerst |

## 5. systemd

Unit: `/etc/systemd/system/martuni-schleuse-worker.service`

```bash
sudo systemctl status  martuni-schleuse-worker.service
sudo systemctl restart martuni-schleuse-worker.service
journalctl -u martuni-schleuse-worker.service -f
tail -f /var/www/but2/botdir/enginemartuni/logs/schleuse_worker.log
```

Alle Pfade und Engine-Labels stehen in `schleuse-work/worker.env`
(`EnvironmentFile`), nicht in der Unit und nicht im Quelltext. `git pull` im
Repo kollidiert deshalb nicht mit der Host-Konfiguration.

**Nie zwei Worker gleichzeitig.** Der Worker hält ein PID-Lock und beendet
sich mit rc=0, wenn schon einer läuft — vor einem manuellen Lauf trotzdem
erst `systemctl stop`.

Ein Cron-Keepalive ist **nicht** eingerichtet und wird nicht gebraucht:
`Restart=always` mit `RestartSec=10` erfüllt denselben Zweck ohne das Risiko
eines zweiten Workers.

## 6. Koexistenz mit dem Voigtsbach-Bot

Auf diesem Host läuft parallel `lichess-bot-voigtsbach.service` (Engine
Funken). `BETRIEB-lichess-bot.md` in der Schleuse warnt in Abschnitt 10
ausdrücklich davor, Analyse und Bot auf demselben Rechner zu fahren, „wenn die
Kerne knapp sind" — genau daran ist der Martuni-Server gescheitert.

Hier ist die Lage anders, und das ist der Grund, warum es trotzdem geht:

| | Martuni-Server (gescheitert) | dieser Host |
|---|---|---|
| Kerne | 2 | 4 |
| Bot-`concurrency` | 2 | 1 |
| Analyse-Threads | 1 | 2 |
| freie Kerne für den Bot | 0 | 2 |

Der Bot hat Vorrang, weil Funken unter Zeitdruck spielt und die Analyse nicht.
Die Unit setzt dafür `Nice=10`, `CPUWeight=20` und `IOSchedulingClass=idle`
(`ionice` gehörte auch zum damaligen Ansatz auf dem Martuni-Server).

**Messung bei Inbetriebnahme (21.09.2026):** Load 2,49 bei laufender Analyse,
Stockfish 195 % CPU auf Nice 10, Voigtsbach durchgehend `online: true`, null
Fehler im Bot-Journal.

Prüfen, ob es kippt:

```bash
uptime                                     # Load < 4 ist unkritisch
curl -s "https://lichess.org/api/users/status?ids=voigtsbach"
journalctl -u lichess-bot-voigtsbach.service --since "-1h" | grep -cE "HTTPError: 429|Control stream error"
```

Zeigt der Bot Zeitverluste oder Verbindungsprobleme, hat er Vorrang: Analyse
stoppen (`systemctl stop martuni-schleuse-worker.service`), dann `--threads`
in `tools/schleuse_worker.py` senken oder die Analyse in Randzeiten legen.

## 7. Laufzeit

Erfahrungswert des Martuni-Servers: ~10 min pro Partie bei Tiefe 17 — ein
Messwert **jener 2-Kern-VM**, nicht übertragbar. Dieser Host ist deutlich
schneller.

Belastbare Zahlen bitte selbst erheben statt aus Einzelmessungen
hochzurechnen: Die Analysezeit skaliert grob mit der Zugzahl, und die streut
erheblich (in der Varianten-Reihe des Martuni-Servers liegt der letzte
Blunder-Ply bei min 1 / median 39 / max 210 — Faktor 5 zwischen Median und
Maximum, und das ist nur eine Untergrenze der Partielänge). Varianten mit
Fairy sind zudem langsamer als Standard mit Stockfish.

### Messwerte dieses Hosts (21.09.2026, erste 14 Partien)

| Engine | n | Median | Spanne |
|---|---|---|---|
| `stockfish` (Standard, Chess960) | 11 | **46 s** | 22–88 s |
| `fairy-stockfish` (echte Varianten) | 3 | **105 s** | 28–389 s |

Die Trennung nach Engine ist die entscheidende, und die Streuung innerhalb der
Varianten ist grösser als der Unterschied zwischen den Engines: Eine
Crazyhouse-Partie brauchte **389 s**, das 8,5-fache des Stockfish-Medians, eine
Atomic-Partie dagegen nur 28 s. Wer die Warteschlange abschätzt, muss Standard
und Varianten getrennt rechnen — und auch dann bleibt es eine grobe Schätzung.
Eine Zahl über alles führt in die Irre.

Die Stichprobe ist klein; für aktuelle Zahlen das Skript unten laufen lassen,
statt diese Tabelle fortzuschreiben.

Laufzeiten selbst erheben: `tools/schleuse_runtimes.py` (liest
`logs/schleuse_worker.log`, gruppiert nach Engine).

## 8. Fehlersuche

```bash
# Warteschlange und Ergebnisse
ssh martuni.de 'cd grok_bot_schleuse; ls outbox | wc -l; ls inbox; cat status.json'

# Fehlgeschlagene Partien
ssh martuni.de 'cd grok_bot_schleuse; for f in inbox/*.failed.json; do echo "== $f"; cat "$f"; done'

# Worker-Log
grep -E "FAIL|TIMEOUT|EXCEPTION|Loop error" logs/schleuse_worker.log
```

- **`status.json` veraltet:** Worker steht. `systemctl status` prüfen.
- **Viele `.failed.json`:** Fehlertext steht in der Datei. Meist Engine- oder
  Pfadproblem; `worker.env` gegen die tatsächlichen Binärpfade prüfen.
- **Timeout nach 45 min:** Kam auf dem alten Host bei Racing Kings vor. Die
  Partie wird als `.failed.json` gemeldet und aus der outbox entfernt; der
  Martuni-Server bietet sie erneut an, nach 3 Fehlschlägen Quarantäne.
- **`outbox` voll, `inbox` leer:** Wir holen nicht ab — hier nachsehen, nicht
  auf dem Martuni-Server.

## 9. Abgrenzung

- Nur Martuni-Partien. Die Analyse von **Voigtsbach**/Funken-Partien ist eine
  getrennte Pipeline und gehört **nicht** in diese Schleuse.
- Auf dem alten Grok-Bot-Host darf die Schleuse nicht reaktiviert werden,
  solange dieser Host die Aufgabe hat — zwei Worker an derselben outbox
  würden dieselbe PGN doppelt rechnen.
