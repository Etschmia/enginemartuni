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
