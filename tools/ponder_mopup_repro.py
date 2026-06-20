#!/usr/bin/env python3
"""Repro: Mop-up-Konversion unter Live-Bedingungen (KQvK aus zKfpQEn8).

Martuni (Weiss, K+Q) sucht mit echten Clocks; der Verteidiger (Schwarz,
nackter Koenig) antwortet INSTANT mit Zufallszuegen — wie die Live-
Situation, in der der Gegner sofort zieht. Optional Ponder-Mechanik
(go ponder -> ponderhit/stop) und TT-Clear pro Zug (ucinewgame).

Live-Befund: zKfpQEn8 Remis durch 50-Zuege-Regel trotz KQvK, Martuni
verbrauchte ~50ms/Zug (clk-Kommentare) und fuehrte den Koenig nie.

Aufruf: ponder_mopup_repro.py [--ponder] [--clear-tt] [--max N]
"""
import subprocess, sys, threading, queue, time, random
import chess, chess.pgn

ENGINE = "./target/release/martuni"
PGN = "/home/librechat/lichess-bot/game_records/Martuni vs AetherBot - zKfpQEn8.pgn"
random.seed(42)


class Uci:
    def __init__(self):
        self.p = subprocess.Popen([ENGINE], stdin=subprocess.PIPE,
                                  stdout=subprocess.PIPE, text=True, bufsize=1)
        self.q = queue.Queue()
        threading.Thread(target=self._reader, daemon=True).start()

    def send(self, cmd):
        self.p.stdin.write(cmd + "\n")
        self.p.stdin.flush()

    def _reader(self):
        for line in self.p.stdout:
            self.q.put(line.strip())

    def wait_bestmove(self, timeout=30):
        infos = []
        t0 = time.time()
        while time.time() - t0 < timeout:
            try:
                line = self.q.get(timeout=0.1)
            except queue.Empty:
                continue
            if line.startswith("info"):
                infos.append(line)
            if line.startswith("bestmove"):
                parts = line.split()
                pd = parts[3] if len(parts) > 3 and parts[2] == "ponder" else None
                return parts[1], pd, infos
        raise TimeoutError("kein bestmove")

    def drain(self):
        while not self.q.empty():
            self.q.get_nowait()


def last_depth_score(infos):
    d, s = None, None
    for ln in infos:
        if ln.startswith("info depth"):
            toks = ln.split()
            d = toks[2]
            if "score" in toks:
                i = toks.index("score")
                s = f"{toks[i+1]} {toks[i+2]}"
    return d, s


def main():
    use_ponder = "--ponder" in sys.argv
    clear_tt = "--clear-tt" in sys.argv
    max_plies = 140

    # Live-Historie bis zum KQvK-Eintritt (Schwarz am Zug, Weiss = K+Q)
    g = chess.pgn.read_game(open(PGN, errors="replace"))
    bd = g.board()
    hist = []
    for mv in g.mainline_moves():
        bd.push(mv)
        hist.append(mv.uci())
        wp = sum(len(bd.pieces(pt, chess.WHITE)) for pt in
                 (chess.PAWN, chess.KNIGHT, chess.BISHOP, chess.ROOK))
        brest = sum(len(bd.pieces(pt, chess.BLACK)) for pt in
                    (chess.PAWN, chess.KNIGHT, chess.BISHOP, chess.ROOK, chess.QUEEN))
        if wp == 0 and brest == 0 and len(bd.pieces(chess.QUEEN, chess.WHITE)) == 1:
            break
    assert bd.turn == chess.BLACK
    print(f"Start Zug {bd.fullmove_number} (KQvK, Schwarz am Zug) | "
          f"Ponder={'AN' if use_ponder else 'AUS'} TT-Clear={'AN' if clear_tt else 'AUS'}")

    eng = Uci()
    eng.send("uci"); eng.send("isready")
    time.sleep(0.5); eng.drain()

    wclock = 120.0
    inc = 5.0
    pred = None          # Pondermove, auf den die Engine gerade pondert
    martuni_kingmoves = 0
    plies = 0
    while plies < max_plies and not bd.is_game_over(claim_draw=True):
        if bd.turn == chess.BLACK:
            # Verteidiger: instant Zufallszug
            mv = random.choice(list(bd.legal_moves))
            bd.push(mv); hist.append(mv.uci())
            plies += 1
            continue

        # --- Martuni (Weiss) ---
        if clear_tt:
            eng.send("ucinewgame")
        t0 = time.time()
        gocmd = (f"go wtime {int(wclock*1000)} btime 60000 "
                 f"winc {int(inc*1000)} binc {int(inc*1000)}")
        if use_ponder and pred is not None:
            if hist[-1] == pred:
                eng.send("ponderhit")
            else:
                eng.send("stop")
                eng.wait_bestmove()
                eng.drain()
                eng.send("position startpos moves " + " ".join(hist))
                eng.send(gocmd)
        else:
            eng.send("position startpos moves " + " ".join(hist))
            eng.send(gocmd)
        bm, pd, infos = eng.wait_bestmove()
        elapsed = time.time() - t0
        wclock = max(wclock - elapsed + inc, 1.0)
        pred = None

        d, s = last_depth_score(infos)
        mv = chess.Move.from_uci(bm)
        san = bd.san(mv)
        if san.startswith("K"):
            martuni_kingmoves += 1
        print(f"  {bd.fullmove_number:3}. {san:7} {elapsed*1000:6.0f}ms "
              f"d{d or '?':>2} {s or '-'}")
        bd.push(mv); hist.append(bm)
        plies += 1
        if bd.halfmove_clock >= 100:
            print("  -> 50-ZUEGE"); break

        # Ponder auf der vorhergesagten Stellung starten
        if use_ponder and pd is not None and not bd.is_game_over():
            ppos = "position startpos moves " + " ".join(hist) + " " + pd
            try:
                chk = bd.copy(); chk.push(chess.Move.from_uci(pd))
            except Exception:
                pd = None
            if pd:
                eng.send(ppos)
                eng.send(gocmd + " ponder")
                pred = pd

    res = ("MATT" if bd.is_checkmate() else
           "PATT" if bd.is_stalemate() else
           f"offen/Draw hmvc={bd.halfmove_clock}")
    print(f"\nErgebnis: {res} | Halbzuege {plies} | Martuni-Koenigszuege "
          f"{martuni_kingmoves} | Restzeit {wclock:.0f}s")
    eng.send("quit")


if __name__ == "__main__":
    main()
