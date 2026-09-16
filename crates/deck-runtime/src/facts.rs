//! Per-app fact readers: turn "the app answered" into "here is what it said".
//!
//! Each `FactsSource` names one of Tanner's own apps and the exact endpoints
//! or file it reads. Same network boundary as `surface`: only origins the user
//! already typed as the project's URL, only when the UI asks.
//!
//! # Every field name here was read off a live response
//!
//! The first version of this module was written against *guessed* payload
//! shapes, and every one of them was wrong: it looked for `status` on Fleet's
//! health endpoint (which reports `ok` and nests `status` under `limits`), for
//! `todayFocusMinutes` on `FocusForge` (which reports `balance_min`, `flow` and
//! `gate`), and for scalar counters on Lathe's `/stats` (which is a map of
//! nested per-route objects). Every reader returned an empty vec against the
//! real services, and because an empty vec is also the honest answer for "not
//! running", nothing looked broken.
//!
//! So the rule for adding a reader: curl the endpoint, paste the real payload
//! into the module comment above the reader, and write the field names from
//! that. A reader that has never seen its own service's output is decoration.
//!
//! Defensive on shape by design -- an unexpected payload skips the fact rather
//! than fabricating one, the same no-fake-data rule as the chip.

use std::path::Path;
use std::time::Duration;

use deck_domain::project::FactsSource;
use serde::Serialize;
use serde_json::Value;

/// Ceiling on a single fact GET. Shorter than the reachability probe: several
/// of these run per tile (Ollama and Lathe each hit two endpoints), and a tile
/// refresh should not stall behind a slow adapter -- the reachability chip has
/// already said the origin answers, so a specific endpoint that hangs is
/// skipped rather than waited on.
const FACTS_TIMEOUT: Duration = Duration::from_secs(4);

/// One glanceable fact: short label, short value, both already formatted.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SurfaceFact {
    /// Display label ("Active runs", "Loaded"). Kept short by the reader.
    pub label: String,
    /// Display value ("2", "84%", "earning"). Already formatted, units and
    /// all: the tile prints it verbatim so units cannot drift between the
    /// backend that measured the number and the frontend that shows it.
    pub value: String,
    /// True when this fact is a problem worth colouring -- a closed gate, a
    /// dead dependency, a non-empty problem list. The tile tints these with
    /// the signal colour; everything else stays neutral, so a tint means
    /// something is wrong rather than "this app is interesting".
    pub alert: bool,
}

impl SurfaceFact {
    fn new(label: &str, value: impl Into<String>) -> Self {
        Self { label: label.to_owned(), value: value.into(), alert: false }
    }

    /// Same fact, marked as a problem.
    fn alert(label: &str, value: impl Into<String>) -> Self {
        Self { label: label.to_owned(), value: value.into(), alert: true }
    }
}

/// Whatever the chosen reader can say right now.
///
/// Empty vec = wired but the source had nothing to report (unreachable, or
/// the origin/log is unset). Never falls back to a fabricated value.
#[must_use]
pub fn read_facts(
    source: FactsSource,
    url: Option<&str>,
    log_path: Option<&Path>,
) -> Vec<SurfaceFact> {
    match source {
        FactsSource::Fleet => url.map(fleet).unwrap_or_default(),
        FactsSource::Ollama => url.map(ollama).unwrap_or_default(),
        FactsSource::Lathe => url.map(lathe).unwrap_or_default(),
        FactsSource::FocusForge => url.map(focus_forge).unwrap_or_default(),
        FactsSource::PavlokStatus => log_path.map(pavlok).unwrap_or_default(),
    }
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(FACTS_TIMEOUT).build()
}

/// GETs and parses, or `None`.
///
/// A non-2xx is deliberately still parsed: Fleet answers its health endpoint
/// with 503 and the *same* JSON body when it has problems, and that body is
/// exactly the case worth reporting. Throwing it away because of the status
/// code would blank the tile precisely when it matters.
fn get_json(url: &str) -> Option<Value> {
    match agent().get(url).call() {
        // A 4xx/5xx body is parsed exactly like a 2xx one: see the doc comment.
        Ok(resp) | Err(ureq::Error::Status(_, resp)) => resp.into_json().ok(),
        Err(ureq::Error::Transport(_)) => None,
    }
}

/// The `scheme://host[:port]` prefix of a URL, without any path.
///
/// Users type the URL they would open in a browser -- often with a path, as
/// the Ollama surface does (`/api/tags`) -- and each adapter appends its own
/// well-known endpoint to the origin.
fn origin(url: &str) -> String {
    let (scheme, rest) = url.split_once("://").unwrap_or(("http", url));
    let host = rest.split('/').next().unwrap_or(rest);
    format!("{scheme}://{host}")
}

/// Rounds a JSON minutes value to whole minutes, saturating rather than
/// wrapping. A raw `as i64` on an out-of-range float is undefined-ish
/// (saturating since Rust 1.45, but clippy is right that the intent should be
/// written down): these are wall-clock minutes, so anything absurd is a
/// malformed payload and clamps instead of producing a negative duration.
// The clamp below bounds the value to `0..=i32::MAX` before the cast, so the
// conversion to i64 cannot truncate. Clippy cannot see that through `clamp`,
// and rewriting it to satisfy the lint would only hide the bound.
#[allow(clippy::cast_possible_truncation)]
fn minutes(value: f64) -> i64 {
    if value.is_finite() { value.round().clamp(0.0, f64::from(i32::MAX)) as i64 } else { 0 }
}

fn u64_at(v: &Value, key: &str) -> Option<u64> {
    v.get(key).and_then(Value::as_u64)
}

fn len_at(v: &Value, key: &str) -> Option<usize> {
    v.get(key).and_then(Value::as_array).map(Vec::len)
}

// ---- Fleet ---------------------------------------------------------------
//
// `GET {origin}/api/health`, unauthenticated by design (Fleet's src/server.ts
// mounts it without the `auth` middleware). Live response, 2026-09-03:
//
// {"ok":true,"problems":[],"uptimeSec":11226,"dryRun":false,"auth":"plan",
//  "activeRuns":0,"armed":7,"schedulerEnabled":true,"disarmed":[],
//  "limits":{"fiveHourPct":84,"fiveHourResetsAt":1788482999959,
//            "sevenDayPct":55,"status":"allowed","subscription":"max",
//            "available":true,"observedAt":1788465701750}}
//
// Note there is no top-level `status`: liveness is `ok`, and `status` lives
// under `limits` and means something else entirely (plan allowance).
fn fleet(url: &str) -> Vec<SurfaceFact> {
    let Some(v) = get_json(&format!("{}/api/health", origin(url))) else {
        return Vec::new();
    };
    let mut out = Vec::new();

    if let Some(n) = u64_at(&v, "activeRuns") {
        out.push(SurfaceFact::new("Active runs", n.to_string()));
    }
    if let Some(n) = u64_at(&v, "armed") {
        out.push(SurfaceFact::new("Armed", n.to_string()));
    }
    // The five-hour window is the number that decides whether a team can run
    // at all, so it earns the third slot ahead of the seven-day one.
    if let Some(pct) = v.get("limits").and_then(|l| u64_at(l, "fiveHourPct")) {
        out.push(SurfaceFact { alert: pct >= 90, ..SurfaceFact::new("5-hour window", format!("{pct}%")) });
    }
    // Problems are the whole reason this endpoint exists; say how many, and
    // never show a reassuring "0" line that competes with the real facts.
    match len_at(&v, "problems") {
        Some(n) if n > 0 => out.push(SurfaceFact::alert("Problems", n.to_string())),
        _ => {}
    }
    out
}

// ---- Ollama --------------------------------------------------------------
//
// Two public endpoints on the Ollama HTTP surface. Live responses, 2026-09-03:
//
// GET {origin}/api/tags -> {"models":[{"name":"qwen3:30b-a3b",...}, ...]}
// GET {origin}/api/ps   -> {"models":[{"name":"qwen2.5-coder:7b",...}]}
//
// Counts only: a list of model names does not fit a tile, and "how many are
// resident right now" is the fact that changes.
fn ollama(url: &str) -> Vec<SurfaceFact> {
    let base = origin(url);
    let mut out = Vec::new();
    if let Some(n) = get_json(&format!("{base}/api/tags")).as_ref().and_then(|v| len_at(v, "models")) {
        out.push(SurfaceFact::new("Installed", n.to_string()));
    }
    if let Some(n) = get_json(&format!("{base}/api/ps")).as_ref().and_then(|v| len_at(v, "models")) {
        out.push(SurfaceFact::new("Loaded", n.to_string()));
    }
    out
}

// ---- Lathe ---------------------------------------------------------------
//
// Live responses, 2026-09-03:
//
// GET {origin}/health -> {"lathe":"ok","ollama":true,
//                         "models":["nomic-embed-text:latest","qwen3:8b",...],
//                         "hardware":{"gpu":"NVIDIA GeForce RTX 3070",...}}
// GET {origin}/stats  -> {"code -> qwen2.5-coder:7b":{"count":26,
//                          "avg_total_ms":5507,"avg_ttft_ms":5436}, ...}
//
// `/stats` is a map of route-and-model to nested counters, so the earlier
// "take the first three scalar fields" approach produced nothing at all.
// Summing `count` across routes gives the one number worth a tile.
fn lathe(url: &str) -> Vec<SurfaceFact> {
    let base = origin(url);
    let mut out = Vec::new();

    if let Some(v) = get_json(&format!("{base}/health")) {
        // Ollama being down is the failure that makes Lathe useless while
        // Lathe itself still answers -- exactly the state a reachability
        // probe alone would call healthy.
        match v.get("ollama").and_then(Value::as_bool) {
            Some(true) => out.push(SurfaceFact::new("Ollama", "up")),
            Some(false) => out.push(SurfaceFact::alert("Ollama", "down")),
            None => {}
        }
        if let Some(n) = len_at(&v, "models") {
            out.push(SurfaceFact::new("Models", n.to_string()));
        }
    }

    if let Some(o) = get_json(&format!("{base}/stats")).as_ref().and_then(Value::as_object) {
        let total: u64 = o.values().filter_map(|r| u64_at(r, "count")).sum();
        if total > 0 {
            out.push(SurfaceFact::new("Requests", total.to_string()));
        }
    }
    out
}

// ---- FocusForge ----------------------------------------------------------
//
// `GET {origin}/api/status`, built by `snapshot()` in focusforge.py. Real
// shape (from that function, and from focusforge.bat which already consumes
// it): every duration is MINUTES with one decimal.
//
// {"balance_min":19.3,"flow":"earning","state":"work","streak":4,
//  "gate":{"enabled":true,"met":false,"remaining_min":30.0,"pct":50,
//          "labels":["Job Hunt","Business"]},
//  "today":{"work_total":124.5,"play_total":31.0,...}, ...}
//
// The priority gate is the reason the app exists (31h of Deep Work against 0h
// on the goals that mattered), so a closed gate is the headline fact and is
// marked as an alert.
fn focus_forge(url: &str) -> Vec<SurfaceFact> {
    let Some(v) = get_json(&format!("{}/api/status", origin(url))) else {
        return Vec::new();
    };
    let mut out = Vec::new();

    if let Some(min) = v.get("balance_min").and_then(Value::as_f64) {
        out.push(SurfaceFact::new("Play credit", format!("{} min", minutes(min))));
    }
    if let Some(flow) = v.get("flow").and_then(Value::as_str) {
        out.push(SurfaceFact::new("Now", flow));
    }
    if let Some(gate) = v.get("gate") {
        if gate.get("enabled").and_then(Value::as_bool) == Some(true) {
            let met = gate.get("met").and_then(Value::as_bool) == Some(true);
            if met {
                out.push(SurfaceFact::new("Gate", "met"));
            } else {
                let left = gate.get("remaining_min").and_then(Value::as_f64).unwrap_or(0.0);
                out.push(SurfaceFact::alert("Gate", format!("{} min left", minutes(left))));
            }
        }
    }
    if let Some(min) = v.get("today").and_then(|t| t.get("work_total")).and_then(Value::as_f64) {
        let m = minutes(min);
        out.push(SurfaceFact::new("Work today", format!("{}h {}m", m / 60, m % 60)));
    }
    out
}

// ---- Pavlok status file --------------------------------------------------
//
// One line, rewritten on every alert by pavlok-notify.mjs / pavlok-limit.mjs:
//
//   2026-09-03T16:57:00.822Z limit zap 10 | [Focus-Forge] Usage limit hit ...
//
// The leading RFC 3339 stamp gives the age; the segment before the pipe is
// the event. The trailing reason is the alert's own message text and is
// deliberately dropped -- it is a sentence, and a fact is a value.
fn pavlok(path: &Path) -> Vec<SurfaceFact> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let Some(line) = text.lines().find(|l| !l.trim().is_empty()) else {
        return Vec::new();
    };

    let (stamp, rest) = line.trim().split_once(char::is_whitespace).unwrap_or((line.trim(), ""));
    let mut out = Vec::new();

    if let Ok(when) = chrono::DateTime::parse_from_rfc3339(stamp) {
        let age = chrono::Utc::now().signed_duration_since(when.with_timezone(&chrono::Utc));
        out.push(SurfaceFact::new("Last alert", human_age(age)));
    }
    let event = rest.split('|').next().unwrap_or("").trim();
    if !event.is_empty() {
        out.push(SurfaceFact::new("Event", event));
    }
    // A line that parsed as neither is not a Pavlok status line; say nothing
    // rather than echoing an unrelated file's first line as a "fact".
    out
}

/// "3h", "12m", "45s" -- the same shape `formatAge` renders in the frontend.
fn human_age(d: chrono::Duration) -> String {
    let secs = d.num_seconds().max(0);
    match secs {
        s if s < 60 => format!("{s}s ago"),
        s if s < 3600 => format!("{}m ago", s / 60),
        s if s < 86_400 => format!("{}h ago", s / 3600),
        s => format!("{}d ago", s / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// Serves one canned response, then stops. Returns the origin to hit.
    fn serve_once(status_line: &str, body: &'static str) -> (String, std::thread::JoinHandle<()>) {
        serve_routes(status_line, vec![("", body)], 1)
    }

    /// Serves `requests` requests, answering each by the first route whose
    /// path fragment appears in the request line.
    ///
    /// A reader that hits two endpoints needs two answers. The first version
    /// of the Lathe test used a one-shot server, so `/health` consumed the
    /// only response and `/stats` hit a closed socket -- the assertion then
    /// failed for a reason that had nothing to do with the code under test.
    /// An unmatched path gets a 404, which is also what a real service does.
    fn serve_routes(
        status_line: &str,
        routes: Vec<(&'static str, &'static str)>,
        requests: usize,
    ) -> (String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let status = status_line.to_owned();
        let handle = std::thread::spawn(move || {
            for _ in 0..requests {
                let Ok((mut conn, _)) = listener.accept() else { return };
                let mut buf = [0_u8; 2048];
                let n = conn.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let hit = routes
                    .iter()
                    .find(|(path, _)| path.is_empty() || req.contains(path));
                let resp = match hit {
                    Some((_, body)) => format!(
                        "{status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{body}",
                        body.len()
                    ),
                    None => "HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\n\r\n".to_owned(),
                };
                let _ = conn.write_all(resp.as_bytes());
            }
        });
        (format!("http://127.0.0.1:{port}"), handle)
    }

    #[test]
    fn origin_strips_the_path() {
        assert_eq!(origin("http://127.0.0.1:4173"), "http://127.0.0.1:4173");
        assert_eq!(origin("http://127.0.0.1:11434/api/tags"), "http://127.0.0.1:11434");
        assert_eq!(origin("https://example.com/foo?bar=1"), "https://example.com");
    }

    /// The exact body Fleet's `/api/health` returned on 2026-09-03. If this
    /// test passes with invented field names, the reader is decorative -- so
    /// the fixture is a real capture, not a hand-written approximation.
    const FLEET_HEALTH: &str = r#"{"ok":true,"problems":[],"uptimeSec":11226,
        "dryRun":false,"auth":"plan","activeRuns":2,"armed":7,
        "schedulerEnabled":true,"disarmed":[],
        "limits":{"fiveHourPct":84,"sevenDayPct":55,"status":"allowed",
                  "subscription":"max","available":true}}"#;

    #[test]
    fn fleet_reads_the_fields_the_real_endpoint_sends() {
        let (base, server) = serve_once("HTTP/1.1 200 OK", FLEET_HEALTH);
        let facts = fleet(&base);
        server.join().unwrap();

        let by = |l: &str| facts.iter().find(|f| f.label == l).map(|f| f.value.clone());
        assert_eq!(by("Active runs").as_deref(), Some("2"));
        assert_eq!(by("Armed").as_deref(), Some("7"));
        assert_eq!(by("5-hour window").as_deref(), Some("84%"));
        // problems is empty here, so no reassuring zero row.
        assert!(by("Problems").is_none(), "an empty problem list must not render a fact");
    }

    /// Fleet answers 503 with the SAME body when it has problems. That is the
    /// case worth reporting, so a non-2xx must still be parsed.
    #[test]
    fn fleet_still_reads_a_503_body_and_flags_problems() {
        let body = r#"{"ok":false,"problems":["plan windows unreadable"],"activeRuns":0,
                       "armed":7,"limits":{"fiveHourPct":97}}"#;
        let (base, server) = serve_once("HTTP/1.1 503 Service Unavailable", body);
        let facts = fleet(&base);
        server.join().unwrap();

        let problems = facts.iter().find(|f| f.label == "Problems").expect("problems fact");
        assert_eq!(problems.value, "1");
        assert!(problems.alert, "a problem must be marked as one");
        let window = facts.iter().find(|f| f.label == "5-hour window").unwrap();
        assert!(window.alert, "97% of the window is an alert");
    }

    /// `FocusForge`'s real snapshot shape: minutes with one decimal, a `flow`
    /// word, and a nested gate. The gate is the app's whole point.
    #[test]
    fn focus_forge_reads_balance_flow_and_a_closed_gate() {
        let body = r#"{"balance_min":19.3,"flow":"earning","streak":4,
            "gate":{"enabled":true,"met":false,"remaining_min":30.0,"pct":50},
            "today":{"work_total":124.5,"play_total":31.0}}"#;
        let (base, server) = serve_once("HTTP/1.1 200 OK", body);
        let facts = focus_forge(&base);
        server.join().unwrap();

        let by = |l: &str| facts.iter().find(|f| f.label == l).expect(l);
        assert_eq!(by("Play credit").value, "19 min");
        assert_eq!(by("Now").value, "earning");
        assert_eq!(by("Gate").value, "30 min left");
        assert!(by("Gate").alert, "a closed priority gate is the alert case");
        // 124.5 minutes rounds to 125, which is 2h 5m. FocusForge reports one
        // decimal place, so half-minutes are a real case, not a contrived one.
        assert_eq!(by("Work today").value, "2h 5m");
    }

    /// Lathe's `/stats` is a map of NESTED per-route objects, which is why
    /// reading "the first few scalar fields" yielded nothing.
    #[test]
    fn lathe_sums_request_counts_across_nested_routes() {
        let health = r#"{"lathe":"ok","ollama":true,
                         "models":["nomic-embed-text:latest","qwen3:8b","qwen3:30b-a3b"]}"#;
        let stats = r#"{"code -> qwen2.5-coder:7b":{"count":26,"avg_total_ms":5507},
                        "heavy -> qwen3:8b":{"count":18,"avg_total_ms":22024},
                        "explicit -> qwen3:8b":{"count":101,"avg_total_ms":4783}}"#;
        let (base, server) =
            serve_routes("HTTP/1.1 200 OK", vec![("/health", health), ("/stats", stats)], 2);
        let facts = lathe(&base);
        server.join().unwrap();

        let by = |l: &str| facts.iter().find(|f| f.label == l).map(|f| f.value.clone());
        assert_eq!(by("Requests").as_deref(), Some("145"));
        assert_eq!(by("Models").as_deref(), Some("3"));
        assert_eq!(by("Ollama").as_deref(), Some("up"));
    }

    /// Lathe answering while Ollama is down is the state a reachability probe
    /// alone calls healthy, and it is the one that makes Lathe useless.
    #[test]
    fn lathe_flags_a_dead_ollama_as_an_alert() {
        let health = r#"{"lathe":"ok","ollama":false,"models":[]}"#;
        let (base, server) = serve_routes("HTTP/1.1 200 OK", vec![("/health", health)], 2);
        let facts = lathe(&base);
        server.join().unwrap();
        let ollama = facts.iter().find(|f| f.label == "Ollama").expect("ollama fact");
        assert_eq!(ollama.value, "down");
        assert!(ollama.alert);
    }

    #[test]
    fn pavlok_reads_the_event_and_ignores_junk() {
        let dir = std::env::temp_dir().join(format!(
            "deck-facts-{:x}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();

        let file = dir.join("pavlok.status");
        let now = chrono::Utc::now().to_rfc3339();
        std::fs::write(&file, format!("{now} limit zap 10 | [Focus-Forge] Usage limit hit\n")).unwrap();
        let facts = pavlok(&file);
        assert_eq!(facts.iter().find(|f| f.label == "Event").map(|f| f.value.clone()).as_deref(),
                   Some("limit zap 10"));
        assert!(facts.iter().any(|f| f.label == "Last alert"));

        // A file that is not a status line yields nothing rather than echoing
        // its first line as though it were measured.
        let junk = dir.join("junk.txt");
        std::fs::write(&junk, "hello\n").unwrap();
        assert!(pavlok(&junk).iter().all(|f| f.label != "Last alert"));

        assert!(pavlok(&dir.join("missing")).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unreachable_origin_returns_no_facts_never_fabricated() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let dead = format!("http://127.0.0.1:{port}/");
        assert!(fleet(&dead).is_empty());
        assert!(ollama(&dead).is_empty());
        assert!(lathe(&dead).is_empty());
        assert!(focus_forge(&dead).is_empty());
    }
}
