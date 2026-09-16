//! Readiness: telling "the process exists" apart from "the thing is serving".
//!
//! # Why this matters more than it sounds
//!
//! A project is marked Running the instant its process is spawned -- 11 ms
//! after the click. But `npm run dev` takes several seconds to compile and bind
//! a port. For that whole window the UI says Running, "open in browser" fails,
//! and the launcher looks broken *because it told the truth too early*. A
//! status that is right about the process and wrong about the service is worse
//! than no status, because people act on it.
//!
//! # Why this needs no HTTP client, no new dependency, and no polling
//!
//! The metrics sampler already walks each project's process tree once a second
//! and records which TCP ports that tree is **listening** on, via
//! `GetExtendedTcpTable`. That is a stronger readiness signal than an HTTP
//! probe and it is already being collected: a process that has bound its port
//! has finished starting, and unlike an HTTP request it costs nothing extra and
//! cannot itself perturb the thing it measures.
//!
//! So readiness is evaluated *from the sample that was taken anyway*, plus the
//! log ring for tools that print a ready line before binding anything
//! predictable. Adding `reqwest` to poll a URL would have meant a TLS stack and
//! a few hundred kilobytes to learn something already on hand.
//!
//! # Unknown is not unhealthy
//!
//! [`Readiness::Unknown`] exists for projects with no probe and no ports -- a
//! script, a test run, a build. Reporting those as unhealthy would put a
//! warning on every batch job that is behaving perfectly.

use deck_domain::manifest::HealthProbe;

/// The verdict for one project at one instant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Readiness {
    /// Confirmed serving: the declared signal was observed.
    Ready,
    /// Expected to become ready but has not yet -- still starting.
    Waiting,
    /// Nothing to check. Not a problem, and must never render as a failure.
    Unknown,
}

impl Readiness {
    /// Maps onto `RunState::Running { healthy }`.
    ///
    /// `Unknown` deliberately becomes `None` rather than `Some(true)`: the UI
    /// must be able to say nothing at all, instead of claiming a health it
    /// never verified.
    #[must_use]
    pub const fn as_healthy(self) -> Option<bool> {
        match self {
            Self::Ready => Some(true),
            Self::Waiting => Some(false),
            Self::Unknown => None,
        }
    }
}

/// Evaluates readiness from data already collected.
///
/// `listening` is the set of ports the project's process tree has bound, taken
/// straight from the metrics sample. `log_text` is the retained log ring.
/// Neither is fetched here -- this function performs no I/O at all, which is
/// what makes it directly testable.
#[must_use]
pub fn evaluate(
    probe: Option<&HealthProbe>,
    declared_ports: &[u16],
    listening: &[u16],
    log_text: &str,
) -> Readiness {
    match probe {
        Some(HealthProbe::LogContains { text }) => {
            // Case-insensitive: tools are inconsistent about capitalising
            // "Ready"/"ready", and a probe that misses because of one letter
            // leaves a project stuck on "Starting" forever.
            if log_text.to_lowercase().contains(&text.to_lowercase()) {
                Readiness::Ready
            } else {
                Readiness::Waiting
            }
        }

        // An HTTP probe is satisfied by the port being bound. The manifest's
        // URL carries a `{port}` template, so the port is the part that
        // actually distinguishes "starting" from "serving"; issuing a real
        // request would confirm the same fact at the cost of a dependency and a
        // side effect on the server being measured.
        Some(HealthProbe::Http { .. } | HealthProbe::TcpPort { .. }) | None => {
            if declared_ports.is_empty() {
                // No port to wait for. If the tree bound *something*, it is
                // serving; otherwise there is nothing to be ready about.
                return if listening.is_empty() {
                    Readiness::Unknown
                } else {
                    Readiness::Ready
                };
            }

            if declared_ports.iter().any(|p| listening.contains(p)) {
                Readiness::Ready
            } else if listening.is_empty() {
                Readiness::Waiting
            } else {
                // Bound, but not where the manifest guessed. Runner
                // `default_ports` are guesses -- `node.toml` lists 3000, 5173
                // and 8080 because Node projects commonly use *one* -- so a
                // process listening anywhere is serving, and insisting on the
                // guessed port would leave it "Starting" forever.
                Readiness::Ready
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bound_declared_port_is_ready() {
        assert_eq!(evaluate(None, &[5173], &[5173], ""), Readiness::Ready);
    }

    #[test]
    fn nothing_bound_yet_is_waiting_not_ready() {
        // The whole point: the process exists, so the old code said Running.
        assert_eq!(evaluate(None, &[5173], &[], ""), Readiness::Waiting);
    }

    #[test]
    fn a_project_with_no_ports_and_no_sockets_is_unknown() {
        // A script or a build. Reporting these unhealthy would put a warning on
        // every batch job that is behaving perfectly.
        assert_eq!(evaluate(None, &[], &[], ""), Readiness::Unknown);
        assert_eq!(evaluate(None, &[], &[], "").as_healthy(), None);
    }

    #[test]
    fn listening_somewhere_else_still_counts_as_ready() {
        // `default_ports` are guesses. node.toml lists 3000/5173/8080 because
        // Node projects commonly use ONE of them; a project on 4321 is serving
        // and must not sit on "Starting" forever.
        assert_eq!(evaluate(None, &[3000, 5173, 8080], &[4321], ""), Readiness::Ready);
    }

    #[test]
    fn a_log_probe_matches_case_insensitively() {
        let probe = HealthProbe::LogContains {
            text: "ready in".into(),
        };
        assert_eq!(
            evaluate(Some(&probe), &[], &[], "VITE v5  Ready in 431ms"),
            Readiness::Ready,
            "capitalisation must not strand a project on Starting"
        );
        assert_eq!(
            evaluate(Some(&probe), &[], &[], "compiling..."),
            Readiness::Waiting
        );
    }

    #[test]
    fn a_log_probe_ignores_ports_entirely() {
        // The reason LogContains exists: tools that bind an unpredictable port
        // but announce themselves. A bound port must not satisfy a log probe,
        // or the probe was pointless.
        let probe = HealthProbe::LogContains {
            text: "listening on".into(),
        };
        assert_eq!(
            evaluate(Some(&probe), &[3000], &[3000], "still building"),
            Readiness::Waiting
        );
    }

    #[test]
    fn an_http_probe_is_satisfied_by_the_bound_port() {
        let probe = HealthProbe::Http {
            url: "http://localhost:{port}/".into(),
            expect_status: vec![],
        };
        assert_eq!(evaluate(Some(&probe), &[8080], &[8080], ""), Readiness::Ready);
        assert_eq!(evaluate(Some(&probe), &[8080], &[], ""), Readiness::Waiting);
    }

    #[test]
    fn readiness_maps_onto_the_run_state_field() {
        assert_eq!(Readiness::Ready.as_healthy(), Some(true));
        assert_eq!(Readiness::Waiting.as_healthy(), Some(false));
        // Unknown must NOT become Some(true): the UI has to be able to say
        // nothing rather than claim a health nobody verified.
        assert_eq!(Readiness::Unknown.as_healthy(), None);
    }
}
