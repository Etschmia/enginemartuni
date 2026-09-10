# Performance review implementation

Feature branch only. **DO NOT MERGE BEFORE USER APPROVAL.** The tournament
must use the unchanged production Martuni engine. Name Martuni and author
Tobias Brendler are unchanged. No deployment or service changes are part of this work.

## Point 1: depth-only search

`go depth D` without remaining-clock values or movetime has no deadline.
Explicit movetime takes precedence; explicit clock allocation and the legacy
budget for bare `go` remain unchanged. Increment alone is not a remaining clock.
Ponderhit is tracked separately from an absent deadline, so untimed searches
cannot accidentally acquire a clock in `should_stop`. Stop remains independent
of the clock. Existing book, forced-move, verified-mate and tablebase early
returns remain in their existing order. A depth limit can still finish pondering,
and verified mate can finish before that limit, as before this patch.

Deterministic unit tests cover clock selection, repeated untimed stop checks,
explicit stop, timed/untimed ponderhit and forced ponder responses.
`tools/perf_regression.py` runs isolated, bounded depth searches across all nine
backends, requires every requested iteration, and saves nodes/score/root-PV and
bestmove (including ponder). `--compare` checks exact equality against another
run. `--controls` also exercises UCI stop, clocks, ponderhit, forced move and mate;
`--syzygy PATH` adds an optional read-only KRvK root-probe check.

Example (supply a copied candidate built from the feature source):

```sh
nice -n 19 python3 tools/perf_regression.py --engine /path/to/candidate \
  --output /path/to/results.json --controls
```

The runner is a correctness check, not a timing benchmark. It fixes hash at
16 MiB, copies this repository's eval.toml, disables books/tablebases for the
differential suite, clears diagnostic environment switches and uses fresh
processes. Watchdog failures and incomplete depths are failures, not measurements.
For deeper reproduction, edit a copy of the position suite deliberately and
keep the same suite for both candidates. Point 1 intentionally changes termination;
points 2 and 3 must preserve completed iteration results.

All builds/tests in this work use the job artifact CARGO_TARGET_DIR, nice 19,
`--jobs 1`, and unit tests use `--test-threads=1`. Production contention precludes
credible speed/NPS/Elo claims. The pre-existing ignored Chess960 microbenchmark
is not run under load. Actual execution records and binary SHA256/source commits
are stored in the job's progress.md and artifacts.

## Point 2: bounded variant adapter improvements

Variant/Chess960 generators expose capture/drop metadata for the exact entry
just yielded. MovePicker consumes it without rescanning the legal list; standard
MoveGen retains the existing board-based fallback. The generator's cursor,
mask, yielded bits, ordering and stable picker sorts are unchanged.

Atomic, Crazyhouse and Chess960 legal lists now use `OnceLock<Arc<Vec<_>>>`.
Construction still calculates the same board mirrors, checkers, EP and hash;
legal generation occurs at its first consumer. Initialized clones share the
list; a clone of an uninitialized board initializes independently. Child boards
start with a fresh cache. No terminal test moves behind pruning: status and
legal_gen still force the list wherever required. TT cutoffs can avoid it.

Variant SEE plays the same infrastructure move on a raw cloned position and
counts board/pocket material with the same piece weights. It avoids building a
second engine adapter (hash, mirrors, outcomes and legal list) solely for SEE;
no new exchange heuristic or rule implementation is introduced. Differential
tests compare this against the old full-child material calculation for every
legal move in selected EP, explosion, promotion, promoted-piece capture,
pocket/drop, castling and variant terminal-transition positions. Existing
adapter-versus-shakmaty perft and variant search/outcome tests remain enabled.

Residual work is explicit: BoardShak's five generic variants still construct
legal lists/outcomes eagerly, preserving the Antichess single-list terminal
optimization. Arbitrary ChessMove-to-shakmaty translation, other metadata callers,
and some quiescence classification still use linear lookup. Raw SEE move play
is repeated when a capture is searched; Crazyhouse checking drops still build a
child for detection and again for search. Eliminating these remaining costs
would require broader move-handle or child-lifetime changes and is deferred.
No general all-children cache, move limit change, or buffer rewrite is included.

Point 2 validation: 201 unit tests passed, zero failures, one pre-existing
ignored microbenchmark. Release UCI results match point 1 exactly for all 11
cases, both with the checked-in modest depths and with `--standard-depth 5`
(Standard and Chess960). This is correctness evidence, not a speedup claim.
