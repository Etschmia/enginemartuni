//! UCI-Protokolltests gegen die echte Binary (Code-Review 14.11.2026).
//!
//! Jeder Test startet einen eigenen Engine-Prozess mit einer isolierten
//! Konfiguration (`MARTUNI_ENV`): kein Buch, kein Syzygy, 1 MB Hash. So
//! haengen die Tests weder von der lokalen `.env` noch von Buchdateien ab.
//! Alle Wartezeiten sind Timeouts — ein haengender UCI-Thread laesst den
//! Test fehlschlagen statt ihn endlos zu blockieren.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

struct Engine {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
    dir: PathBuf,
}

impl Engine {
    fn start() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "martuni-uci-test-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(dir.join("books")).unwrap();
        let env_file = dir.join("test.env");
        std::fs::write(
            &env_file,
            "HASH_SIZE_MB=1\nBOOK_DIR=books\nBOOK_FILES=none.bin\nBOOK_3CHECK=\nBOOK_KOTH=\n",
        )
        .unwrap();

        let mut child = Command::new(env!("CARGO_BIN_EXE_martuni"))
            .env("MARTUNI_ENV", &env_file)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("engine binary starts");
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        Engine { child, stdin, lines: rx, dir }
    }

    fn send(&mut self, cmd: &str) {
        writeln!(self.stdin, "{cmd}").unwrap();
        self.stdin.flush().unwrap();
    }

    /// Wartet bis `timeout` auf eine Zeile, die mit `prefix` beginnt.
    /// Gibt alle bis dahin gelesenen Zeilen zurueck (die passende zuletzt).
    fn wait_for(&mut self, prefix: &str, timeout: Duration) -> Option<Vec<String>> {
        let deadline = Instant::now() + timeout;
        let mut seen = Vec::new();
        loop {
            let left = deadline.checked_duration_since(Instant::now())?;
            let line = self.lines.recv_timeout(left).ok()?;
            let hit = line.starts_with(prefix);
            seen.push(line);
            if hit {
                return Some(seen);
            }
        }
    }

    /// True, wenn innerhalb von `window` KEINE Zeile mit `prefix` kommt.
    fn silent(&mut self, prefix: &str, window: Duration) -> bool {
        self.wait_for(prefix, window).is_none()
    }

    fn bestmove(&mut self, timeout: Duration) -> Option<String> {
        self.wait_for("bestmove", timeout)
            .and_then(|l| l.last().cloned())
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = writeln!(self.stdin, "quit");
        let _ = self.stdin.flush();
        thread::sleep(Duration::from_millis(50));
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

const SEC: Duration = Duration::from_secs(1);

/// Befund 1: `isready` muss waehrend einer laufenden Suche sofort
/// beantwortet werden, und ein danach gesendetes `stop` muss ankommen.
#[test]
fn isready_and_stop_work_during_search() {
    let mut e = Engine::start();
    e.send("position startpos");
    e.send("go depth 50");
    thread::sleep(Duration::from_millis(200));
    e.send("isready");
    assert!(e.wait_for("readyok", 2 * SEC).is_some(), "readyok waehrend der Suche");
    e.send("stop");
    assert!(e.bestmove(5 * SEC).is_some(), "bestmove nach stop");
}

/// Befund 2: eine Ponder-Suche, die frueh fertig ist (Tiefenlimit), darf
/// ihr Ergebnis erst nach `ponderhit` ausgeben.
#[test]
fn ponder_depth_limit_waits_for_ponderhit() {
    let mut e = Engine::start();
    e.send("position startpos");
    e.send("go ponder depth 1");
    assert!(e.silent("bestmove", SEC / 2), "kein bestmove vor ponderhit");
    e.send("ponderhit");
    assert!(e.bestmove(2 * SEC).is_some(), "bestmove nach ponderhit");
}

/// Befund 2: ein im Ponder-Modus gefundenes Matt wird bis `stop` gehalten.
#[test]
fn ponder_mate_waits_for_stop() {
    let mut e = Engine::start();
    e.send("position fen 7k/8/5KQ1/8/8/8/8/8 w - - 0 1");
    e.send("go ponder wtime 60000 btime 60000");
    assert!(e.silent("bestmove", SEC / 2), "kein bestmove vor stop");
    e.send("stop");
    let bm = e.bestmove(2 * SEC).expect("bestmove nach stop");
    assert!(bm.starts_with("bestmove g6g7"), "{bm}");
}

/// Ein neues `go` waehrend eines gehaltenen Ponder-Ergebnisses darf den
/// UCI-Thread nicht aufhaengen.
#[test]
fn go_after_unreleased_ponder_does_not_hang() {
    let mut e = Engine::start();
    e.send("position startpos");
    e.send("go ponder depth 1");
    assert!(e.silent("bestmove", SEC / 4));
    e.send("go depth 1");
    assert!(e.bestmove(2 * SEC).is_some(), "bestmove der abgebrochenen Ponder-Suche");
    assert!(e.bestmove(2 * SEC).is_some(), "bestmove der neuen Suche");
}

/// Befund 8: `go infinite` laeuft, bis `stop` kommt — nicht nur ~1 s.
#[test]
fn go_infinite_runs_until_stop() {
    let mut e = Engine::start();
    e.send("position startpos");
    e.send("go infinite");
    assert!(e.silent("bestmove", 2 * SEC), "kein bestmove ohne stop");
    e.send("stop");
    assert!(e.bestmove(5 * SEC).is_some(), "bestmove nach stop");
}

/// Befund 8: auch ein sofort fertiges Ergebnis (hier: Matt in 1) wird bei
/// `go infinite` bis zum `stop` gehalten.
#[test]
fn go_infinite_holds_early_result() {
    let mut e = Engine::start();
    e.send("position fen 7k/8/5KQ1/8/8/8/8/8 w - - 0 1");
    e.send("go infinite");
    assert!(e.silent("bestmove", SEC / 2));
    e.send("stop");
    let bm = e.bestmove(2 * SEC).expect("bestmove nach stop");
    assert!(bm.starts_with("bestmove g6g7"), "{bm}");
}

/// Befund 10: Wechsel von SyzygyPath A auf einen anderen gueltigen Pfad B
/// muss die Tabellen neu laden statt sie abzuschalten. Braucht echte
/// Tabellen unter `$MARTUNI_TEST_SYZYGY` oder `~/syzygy/3-4-5`; fehlen sie,
/// wird der Test uebersprungen.
#[test]
fn syzygy_path_switch_reloads_tables() {
    let tables = std::env::var("MARTUNI_TEST_SYZYGY")
        .map(PathBuf::from)
        .or_else(|_| std::env::var("HOME").map(|h| PathBuf::from(h).join("syzygy/3-4-5")))
        .ok()
        .filter(|p| p.is_dir());
    let Some(tables) = tables else {
        eprintln!("uebersprungen: keine Syzygy-Tabellen gefunden");
        return;
    };
    let mut e = Engine::start();
    // Pfad B: Symlink auf dieselben Tabellen — anderer Pfad, gueltiger Inhalt.
    let alias = e.dir.join("tb-alias");
    std::os::unix::fs::symlink(&tables, &alias).unwrap();

    let loaded = "info string Syzygy: tablebases loaded";
    e.send(&format!("setoption name SyzygyPath value {}", tables.display()));
    assert!(e.wait_for(loaded, 20 * SEC).is_some(), "Pfad A laedt");
    e.send(&format!("setoption name SyzygyPath value {}", alias.display()));
    assert!(e.wait_for(loaded, 20 * SEC).is_some(), "Pfad B laedt nach A");
    // Und die Tabellen werden danach wirklich benutzt.
    e.send("position fen 8/8/8/8/8/2k5/8/KQ6 w - - 0 1");
    e.send("go depth 1");
    let lines = e.wait_for("bestmove", 5 * SEC).expect("bestmove");
    assert!(lines.iter().any(|l| l.contains("syzygy root hit")), "{lines:?}");
}

/// Befund 10: nach einem Ladefehler loest dasselbe `setoption` einen neuen
/// Ladeversuch aus (frueher: kein Pfadwechsel erkannt → nichts passierte).
#[test]
fn syzygy_load_failure_can_be_retried() {
    let mut e = Engine::start();
    let missing = e.dir.join("gibt-es-nicht");
    let cmd = format!("setoption name SyzygyPath value {}", missing.display());
    e.send(&cmd);
    let failed = "info string Syzygy: no tablebases loaded";
    assert!(e.wait_for(failed, 5 * SEC).is_some(), "erster Versuch meldet den Fehler");
    e.send(&cmd);
    assert!(e.wait_for(failed, 5 * SEC).is_some(), "zweiter Versuch wird ausgefuehrt");
}
