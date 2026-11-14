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
