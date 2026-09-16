//! Probes for surfaces — the things a tile watches that the supervisor did
//! not spawn: a port some other process opened, a deployed URL, a log file the
//! app writes for itself.
//!
//! # The network boundary, stated once
//!
//! This is the first module in Launch Deck that touches the network, so the
//! rule lives here: **only URLs the user typed into a project's status-source
//! config are ever contacted, and only when the UI asks.** There is no
//! background ticker in the backend, no phone-home, no telemetry. A launcher
//! that quietly makes HTTP requests is a different product from the one this
//! repo promises ("no cloud, no accounts, no telemetry"), and the promise is
//! kept by keeping every request user-configured and user-visible.
//!
//! # Honesty rules
//!
//! - A non-2xx response is still a *response*: report the real status code.
//!   Etsy answers 403 to headless pings; that is "reachable, refusing bots",
//!   not "down", and collapsing the two would lie on the tile.
//! - A transport failure reports its error text, never a fabricated status.
//! - An unmeasured probe is `None` end to end — the tile renders "not wired",
//!   never a made-up zero. Same rule that governed the old GPU counters.

use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::Path;
use std::time::{Duration, Instant, SystemTime};

/// How long a local port probe waits before calling the port closed.
///
/// Loopback either accepts in microseconds or refuses immediately; the timeout
/// only matters for a firewalled or half-open state, and 250 ms keeps a wall
/// of misconfigured tiles from stalling a refresh.
const PORT_TIMEOUT: Duration = Duration::from_millis(250);

/// How long an HTTP ping waits for the full response.
///
/// Deployed checks ride real internet latency; 5 s separates "slow" from
/// "gone" without hanging a refresh behind a dead DNS entry for half a minute.
const HTTP_TIMEOUT: Duration = Duration::from_secs(5);

/// The outcome of one HTTP ping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PingOutcome {
    /// The server answered. Any status counts — a 403 or 500 is a live server
    /// saying something, and the tile shows the real code.
    Responded {
        /// HTTP status code as received.
        status: u16,
        /// Request start to response completion, whole milliseconds.
        ms: u32,
        /// The `Content-Type` media type (parameters stripped), when sent.
        /// `text/html` is a page the embed pane can frame; `application/json`
        /// is an API the pane must not pretend is one.
        content_type: Option<String>,
    },
    /// No response: DNS failure, refused connection, TLS error, timeout.
    /// Carries the transport error's own text so the tile can say why.
    Failed(String),
}

/// Whether something is accepting connections on `127.0.0.1:port` right now.
///
/// A *connect*, not a scan of the listen table: "something answers" is the
/// honest signal for "the panel is up", and it costs one syscall against the
/// loopback interface.
#[must_use]
pub fn probe_port(port: u16) -> bool {
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    TcpStream::connect_timeout(&addr, PORT_TIMEOUT).is_ok()
}

/// GETs `url` and reports what actually happened.
///
/// Blocking — callers inside the app go through `spawn_blocking`. The response
/// body is not read beyond what the client buffers; status and latency are the
/// signal, and downloading a deployed SPA's HTML to throw it away would be
/// waste dressed as thoroughness.
#[must_use]
pub fn ping_url(url: &str) -> PingOutcome {
    let started = Instant::now();
    let agent = ureq::AgentBuilder::new()
        .timeout(HTTP_TIMEOUT)
        // Redirects are followed (vercel apex -> www and the like); ten is
        // ureq's own ceiling and fine here.
        .build();

    match agent.get(url).call() {
        Ok(resp) => PingOutcome::Responded {
            status: resp.status(),
            ms: elapsed_ms(started),
            content_type: media_type(&resp),
        },
        // ureq models 4xx/5xx as errors; for a tile they are answers.
        Err(ureq::Error::Status(status, resp)) => PingOutcome::Responded {
            status,
            ms: elapsed_ms(started),
            content_type: media_type(&resp),
        },
        Err(ureq::Error::Transport(t)) => PingOutcome::Failed(t.to_string()),
    }
}

fn elapsed_ms(started: Instant) -> u32 {
    u32::try_from(started.elapsed().as_millis()).unwrap_or(u32::MAX)
}

/// The media type alone: `text/html; charset=utf-8` reports as `text/html`.
///
/// ureq already strips parameters in `content_type()`; this exists so an
/// absent header is `None` rather than ureq's empty string, which the tile
/// would otherwise have to special-case.
fn media_type(resp: &ureq::Response) -> Option<String> {
    let mime = resp.content_type().trim().to_ascii_lowercase();
    (!mime.is_empty()).then_some(mime)
}

/// When `path` was last written, if it exists and says.
///
/// The whole status signal for a log the supervisor does not own: "fleet.log
/// changed 40 seconds ago" is evidence of life that needs no process handle.
#[must_use]
pub fn log_mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok()?.modified().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    #[test]
    fn an_open_port_probes_true_and_a_closed_one_false() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(probe_port(port), "listening port should probe true");

        drop(listener);
        assert!(!probe_port(port), "freed port should probe false");
    }

    /// A live server answering 200 is reported with its real status and a
    /// plausible latency — the whole point of the tile's number.
    #[test]
    fn a_responding_server_reports_status_and_latency() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut conn, _) = listener.accept().unwrap();
            let mut buf = [0_u8; 1024];
            let _ = conn.read(&mut buf);
            conn.write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\n\r\nok")
                .unwrap();
        });

        let outcome = ping_url(&format!("http://127.0.0.1:{port}/"));
        server.join().unwrap();
        match outcome {
            PingOutcome::Responded { status, ms, .. } => {
                assert_eq!(status, 200);
                assert!(u128::from(ms) < HTTP_TIMEOUT.as_millis());
            }
            PingOutcome::Failed(e) => panic!("expected a response, got {e}"),
        }
    }

    /// Non-2xx is an ANSWER. Collapsing a 503 into "down" would erase the
    /// difference between a dead server and a live one refusing — the exact
    /// distinction the Etsy tile needs.
    #[test]
    fn a_non_2xx_response_reports_its_real_code() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut conn, _) = listener.accept().unwrap();
            let mut buf = [0_u8; 1024];
            let _ = conn.read(&mut buf);
            conn.write_all(b"HTTP/1.1 503 Service Unavailable\r\ncontent-length: 0\r\n\r\n")
                .unwrap();
        });

        let outcome = ping_url(&format!("http://127.0.0.1:{port}/"));
        server.join().unwrap();
        assert!(
            matches!(outcome, PingOutcome::Responded { status: 503, .. }),
            "got {outcome:?}"
        );
    }

    #[test]
    fn a_refused_connection_fails_with_a_reason() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        match ping_url(&format!("http://127.0.0.1:{port}/")) {
            PingOutcome::Failed(reason) => assert!(!reason.is_empty()),
            other @ PingOutcome::Responded { .. } => {
                panic!("expected failure, got {other:?}")
            }
        }
    }

    #[test]
    fn log_mtime_reads_a_real_file_and_none_for_a_missing_one() {
        let dir = std::env::temp_dir().join(format!(
            "deck-surface-{:x}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.log");
        std::fs::write(&file, "x").unwrap();

        assert!(log_mtime(&file).is_some());
        assert!(log_mtime(&dir.join("missing.log")).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
