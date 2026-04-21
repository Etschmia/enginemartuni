

sudo systemctl status lichess-bot         # Momentaufnahme
sudo journalctl -u lichess-bot -f         # Live-Log
sudo systemctl restart lichess-bot        # z.B. nach config.yml- oder Engine-Rebuild
Wichtig zum Merken: Nach jedem cargo build --release musst du sudo systemctl restart lichess-bot ausführen, damit der Bot die neue Engine-Binary lädt — sonst redet er weiter mit der alten.

Martuni hat am 14.04.2026 um 16:18 folgendes Rating:
Blitz 1530
Schnellschach 1680
Martuni hat am 15.04.2026 um 20:18 folgendes Rating:
Blitz 1659
Schnellschach 1756
Martuni hat am 16.04.2026 um 16:00 folgendes Rating:
Blitz 1662
Schnellschach 1771
Martuni hat am 21.04.2026 um 09:00 folgendes Rating:
Blitz 1733
Schnellschach 1842


Vor dem Fix
Nach dem Fix spielt Martuni als erstes gegen BOT OmbleCavalierPP (1867) und läuft mit 26. ...Lh4 direkt in eine Gabel die ihn die Dame kostet. Bis dahin stand er prima .... Sowas muss mak eigentlich sehen


python3 tools/analyze_blunders.py \
  --game-dir ../lichess-bot/game_records/ \
  --since 2026-04-14T14:45 \
  --min-movetime 0.3 \
  --depth 17 \
  --threads 2 \
  --hash 256 > analyse_16.04.2026.txt

Nohup- Variante
cd /home/librechat/enginemartuni
source .venv/bin/activate
nohup python3 tools/analyze_blunders.py \
  --game-dir ../lichess-bot/game_records/ \
  --min-movetime 0.3 \
  --depth 17 \
  --threads 2 \
  --hash 256 > analyse_20.04.2026.txt 2>&1 &
