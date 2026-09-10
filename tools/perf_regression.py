#!/usr/bin/env python3
"""Bounded UCI correctness comparison. Never interprets contended time as performance.

Run with nice -n 19 python3 tools/perf_regression.py --engine COPY --output result.json.
Use --compare previous.json for exact completed iteration nodes/score/PV/bestmove.
Books and tablebases are disabled in an isolated directory; each case gets a fresh TT.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import queue
import shutil
import subprocess
import tempfile
import threading
import time


class Engine:
    def __init__(self, binary, directory):
        env = {k: v for k, v in os.environ.items() if not k.startswith('MARTUNI_')}
        self.p = subprocess.Popen([str(binary)], cwd=directory, env=env,
                                  stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                  stderr=subprocess.STDOUT, text=True, bufsize=1)
        self.lines = queue.Queue()
        def read():
            for line in self.p.stdout:
                self.lines.put(line.strip())
            self.lines.put(None)
        threading.Thread(target=read, daemon=True).start()
        self.send('uci')
        startup = self.until('uciok')
        assert 'id name Martuni' in startup and 'id author Tobias Brendler' in startup, startup
        assert not any('book loaded' in x.lower() for x in startup), startup
        self.send('setoption name Hash value 16')
        self.send('setoption name MoveOverhead value 0')
        self.send('isready')
        self.until('readyok')

    def send(self, line):
        self.p.stdin.write(line + '\n')
        self.p.stdin.flush()

    def until(self, prefix, timeout=120):
        result = []
        end = time.monotonic() + timeout
        while True:
            line = self.lines.get(timeout=max(.001, end-time.monotonic()))
            assert line is not None, result
            result.append(line)
            if line.startswith(prefix):
                return result

    def close(self):
        if self.p.poll() is None:
            self.send('quit')
            try:
                self.p.wait(timeout=10)
            except subprocess.TimeoutExpired:
                self.p.kill()  # Only the child created here.
                self.p.wait()
        self.p.stdin.close()
        self.p.stdout.close()


def iterations(lines):
    result = []
    for line in lines:
        t = line.split()
        if t[:2] != ['info', 'depth']:
            continue
        i = t.index('score')
        result.append({'depth': int(t[2]), 'nodes': int(t[t.index('nodes')+1]),
                       'score': t[i+1:i+3], 'pv': t[t.index('pv')+1:] if 'pv' in t else []})
    return result


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--engine', type=Path, required=True)
    ap.add_argument('--output', type=Path, required=True)
    ap.add_argument('--compare', type=Path)
    ap.add_argument('--controls', action='store_true', help='Also check stop, ponderhit, clocks, mate, forced move')
    ap.add_argument('--syzygy', type=Path, help='Optional read-only tablebase root control test')
    a = ap.parse_args()
    root = Path(__file__).resolve().parents[1]
    results = {'engine_sha256': hashlib.sha256(a.engine.read_bytes()).hexdigest(),
               'eval_sha256': hashlib.sha256((root/'eval.toml').read_bytes()).hexdigest(),
               'kind': 'correctness only; no timing measurement', 'cases': {}}
    with tempfile.TemporaryDirectory(prefix='martuni-regression-') as tmp:
        d = Path(tmp)
        binary = d/'martuni'
        shutil.copy2(a.engine, binary)
        shutil.copy2(root/'eval.toml', d/'eval.toml')
        (d/'books').mkdir()
        config = 'HASH_SIZE_MB=16\nBOOK_DIR=books\nBOOK_FILES=absent.bin\nSYZYGY_PATH=\n'
        (d/'.env').write_text(config)
        for case in json.loads((root/'tests/perf_positions.json').read_text()):
            e = Engine(binary, d)
            try:
                e.send('setoption name UCI_Variant value '+case['variant'])
                e.send('setoption name UCI_Chess960 value '+str(case['chess960']).lower())
                e.send('position '+case['position'])
                e.send('go depth '+str(case['depth']))
                lines = e.until('bestmove')
                it = iterations(lines)
                assert [x['depth'] for x in it] == list(range(1,case['depth']+1)), lines
                results['cases'][case['id']] = {'iterations': it, 'bestmove': lines[-1]}
            finally:
                e.close()
        if a.controls:
            controls = [
                ('stop', 'startpos', 'go depth 64', 'stop', None),
                ('clock', 'startpos', 'go depth 64 wtime 1 btime 1', None, None),
                ('movetime', 'startpos', 'go depth 64 movetime 1', None, None),
                ('ponderhit', 'startpos', 'go ponder depth 64 movetime 1', 'ponderhit', None),
                ('ponder-stop', 'startpos', 'go ponder depth 64', 'stop', None),
                ('forced', 'fen 7k/8/5K2/8/8/8/8/7R b - - 0 1', 'go depth 64', None, 'forced move'),
                ('mate', 'fen 7k/8/5KQ1/8/8/8/8/8 w - - 0 1', 'go depth 4', None, 'score mate 1'),
            ]
            if a.syzygy:
                controls.append(('syzygy', 'fen 7k/8/8/8/8/4K3/8/R7 w - - 0 1', 'go depth 64', None, 'syzygy root hit'))
            results['controls'] = {}
            for name, pos, go, signal, expected in controls:
                if name == 'syzygy':
                    (d/'.env').write_text(config.replace('SYZYGY_PATH=','SYZYGY_PATH='+str(a.syzygy.resolve())))
                e = Engine(binary, d)
                try:
                    e.send('position '+pos)
                    e.send(go)
                    if signal:
                        first = e.until('info depth')
                        assert not any(x.startswith('bestmove') for x in first), first
                        e.send(signal)
                    lines = e.until('bestmove')
                    if expected:
                        assert any(expected in x for x in lines), lines
                    results['controls'][name] = 'passed'
                finally:
                    e.close()
    if a.compare:
        previous = json.loads(a.compare.read_text())
        assert previous['eval_sha256'] == results['eval_sha256']
        assert previous['cases'] == results['cases'], 'Iteration nodes/score/PV/bestmove differ'
        results['compared_to'] = str(a.compare)
    a.output.write_text(json.dumps(results, indent=2)+'\n')
    print('PASS:', a.output)


if __name__ == '__main__':
    main()
