//! Listening-port discovery for a process tree.
//!
//! Two jobs, both wanted by the UI and both needing the same table:
//!
//! * **Which ports is this project serving on?** A dev server's port is the
//!   single most useful thing to show about it, and nothing in the process API
//!   reports it -- it has to come from the TCP table.
//! * **Is the port already taken?** "Port 5173 is already in use" is the most
//!   common dev-server failure there is, and it is far better caught before
//!   spawning than diagnosed afterwards from a stack trace.
//!
//! Both read the system TCP table via `GetExtendedTcpTable`, which is what
//! `netstat -ano` uses: it maps listening ports to owning process ids without
//! running anything.

use std::collections::HashSet;

/// One listening TCP endpoint and the process holding it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Listener {
    /// Port being listened on.
    pub port: u16,
    /// Process id that owns the socket.
    pub pid: u32,
}

/// Every listening TCP socket on the machine, with its owning pid.
///
/// Returns an empty vector rather than an error if the table cannot be read:
/// port information is decoration on top of process state, and losing it must
/// never fail a launch or blank a dashboard.
#[must_use]
pub fn listeners() -> Vec<Listener> {
    listeners_impl().unwrap_or_else(|e| {
        tracing::debug!(error = %e, "could not read the TCP table");
        Vec::new()
    })
}

/// The ports a set of processes is listening on, sorted and deduplicated.
///
/// Takes the whole process tree rather than one pid because the process that
/// binds the port is usually not the one launched -- `npm` starts `node`, and it
/// is `node` that listens.
#[must_use]
pub fn ports_for_pids<S: std::hash::BuildHasher>(pids: &HashSet<u32, S>) -> Vec<u16> {
    let mut ports: Vec<u16> = listeners()
        .into_iter()
        .filter(|l| pids.contains(&l.pid))
        .map(|l| l.port)
        .collect();
    ports.sort_unstable();
    ports.dedup();
    ports
}

/// Whether any process is already listening on `port`.
#[must_use]
pub fn is_port_in_use(port: u16) -> bool {
    listeners().iter().any(|l| l.port == port)
}

/// The first of `ports` that is already taken, if any.
///
/// Used before spawning so a conflict is reported as a conflict instead of
/// surfacing as an opaque exit code once the child has already failed.
#[must_use]
pub fn first_conflict(ports: &[u16]) -> Option<u16> {
    if ports.is_empty() {
        return None;
    }
    first_conflict_in(&taken_ports(), ports)
}

/// Every port currently listening, as one snapshot of the TCP table.
///
/// Enumerating that table is a syscall over every TCP endpoint on the machine,
/// so a caller checking several lists should take ONE snapshot and reuse it.
/// Launching a project used to call [`first_conflict`] twice -- once for the
/// ports the user configured and once for the ports the runner guesses -- which
/// read the whole table twice before anything was spawned.
#[must_use]
pub fn taken_ports() -> HashSet<u16> {
    listeners().into_iter().map(|l| l.port).collect()
}

/// The first of `ports` present in an already-taken snapshot.
#[must_use]
pub fn first_conflict_in<S: std::hash::BuildHasher>(
    taken: &std::collections::HashSet<u16, S>,
    ports: &[u16],
) -> Option<u16> {
    ports.iter().copied().find(|p| taken.contains(p))
}

#[allow(unsafe_code, clippy::cast_possible_truncation)]
fn listeners_impl() -> std::io::Result<Vec<Listener>> {
    use windows::Win32::NetworkManagement::IpHelper::{
        GetExtendedTcpTable, MIB_TCPTABLE_OWNER_PID, TCP_TABLE_OWNER_PID_LISTENER,
    };
    use windows::Win32::Networking::WinSock::AF_INET;

    // Two-call pattern: ask for the size, allocate, then fill. The table can
    // grow between the calls, so retry a bounded number of times.
    let mut size: u32 = 0;
    for _ in 0..5 {
        // SAFETY: a null buffer with a valid size pointer is the documented way
        // to query the required length; nothing is written to the buffer.
        let rc = unsafe {
            GetExtendedTcpTable(
                None,
                &raw mut size,
                false,
                AF_INET.0.into(),
                TCP_TABLE_OWNER_PID_LISTENER,
                0,
            )
        };
        // 122 == ERROR_INSUFFICIENT_BUFFER, the expected answer to a size query.
        if rc != 0 && rc != 122 {
            return Err(std::io::Error::other(format!(
                "GetExtendedTcpTable size query failed: {rc}"
            )));
        }
        if size == 0 {
            return Ok(Vec::new());
        }

        // Align the buffer to the table struct; a Vec<u8> would not guarantee it.
        let mut buffer: Vec<u32> = vec![0; (size as usize).div_ceil(4)];
        // SAFETY: `buffer` is at least `size` bytes and aligned for
        // MIB_TCPTABLE_OWNER_PID, whose first field is a u32 count.
        let rc = unsafe {
            GetExtendedTcpTable(
                Some(buffer.as_mut_ptr().cast()),
                &raw mut size,
                false,
                AF_INET.0.into(),
                TCP_TABLE_OWNER_PID_LISTENER,
                0,
            )
        };
        if rc == 122 {
            continue; // grew between calls; retry with the new size
        }
        if rc != 0 {
            return Err(std::io::Error::other(format!(
                "GetExtendedTcpTable failed: {rc}"
            )));
        }

        // SAFETY: the call succeeded, so the buffer holds a
        // MIB_TCPTABLE_OWNER_PID followed by `dwNumEntries` row structs.
        let table = unsafe { &*buffer.as_ptr().cast::<MIB_TCPTABLE_OWNER_PID>() };
        let count = table.dwNumEntries as usize;
        // SAFETY: the rows are contiguous immediately after the count field;
        // `table.table` is the documented start of that array.
        let rows = unsafe { std::slice::from_raw_parts(table.table.as_ptr(), count) };

        return Ok(rows
            .iter()
            .map(|row| Listener {
                // The port is stored in network byte order.
                port: u16::from_be(row.dwLocalPort as u16),
                pid: row.dwOwningPid,
            })
            .collect());
    }

    Err(std::io::Error::other(
        "TCP table kept growing between size query and read",
    ))
}

/// Free and total bytes on the volume holding `path`.
///
/// Lives beside the other Win32 probes because this crate is the one place the
/// project talks to the platform directly; diagnostics needs it to answer
/// "can this app still write a snapshot".
#[allow(unsafe_code)]
#[must_use]
pub fn disk_space(path: &std::path::Path) -> Option<(u64, u64)> {
    use windows::core::HSTRING;
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

    let root = path.ancestors().last()?;
    let wide = HSTRING::from(root.as_os_str());
    let (mut free, mut total, mut total_free) = (0u64, 0u64, 0u64);
    // SAFETY: all three out-pointers are valid locals, and `wide` owns a valid
    // null-terminated wide string for the duration of the call.
    let ok = unsafe {
        GetDiskFreeSpaceExW(
            &wide,
            Some(std::ptr::addr_of_mut!(free)),
            Some(std::ptr::addr_of_mut!(total)),
            Some(std::ptr::addr_of_mut!(total_free)),
        )
    };
    ok.ok().map(|()| (free, total))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn reads_the_system_tcp_table() {
        // A Windows box always has something listening.
        let found = listeners();
        assert!(!found.is_empty(), "no listeners reported at all");
        assert!(found.iter().all(|l| l.port > 0));
    }

    #[test]
    fn finds_a_port_this_process_is_listening_on() {
        let socket = TcpListener::bind("127.0.0.1:0").expect("bind an ephemeral port");
        let port = socket.local_addr().unwrap().port();

        assert!(is_port_in_use(port), "port {port} was bound but not reported");

        let me: std::collections::HashSet<u32> = [std::process::id()].into_iter().collect();
        assert!(
            ports_for_pids(&me).contains(&port),
            "port {port} not attributed to this process"
        );
    }

    #[test]
    fn a_released_port_stops_being_reported() {
        let port = {
            let socket = TcpListener::bind("127.0.0.1:0").unwrap();
            socket.local_addr().unwrap().port()
        }; // dropped: socket closed
        // TIME_WAIT applies to connections, not idle listeners, so a closed
        // listener disappears immediately.
        assert!(!is_port_in_use(port));
    }

    #[test]
    fn conflict_detection_finds_the_taken_port() {
        let socket = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = socket.local_addr().unwrap().port();

        assert_eq!(first_conflict(&[port]), Some(port));
        // A free port alongside it must not mask the real conflict.
        assert_eq!(first_conflict(&[1, port]), Some(port));
    }

    #[test]
    fn no_ports_means_no_conflict() {
        assert_eq!(first_conflict(&[]), None);
    }

    #[test]
    fn unrelated_pids_own_no_ports() {
        let nobody: std::collections::HashSet<u32> = [0xFFFF_FFF0].into_iter().collect();
        assert!(ports_for_pids(&nobody).is_empty());
    }

    #[test]
    fn results_are_sorted_and_deduplicated() {
        let mut pids = std::collections::HashSet::new();
        pids.insert(std::process::id());
        let sockets: Vec<TcpListener> = (0..3)
            .map(|_| TcpListener::bind("127.0.0.1:0").unwrap())
            .collect();
        let ports = ports_for_pids(&pids);
        assert!(ports.windows(2).all(|w| w[0] < w[1]), "not sorted/deduped: {ports:?}");
        drop(sockets);
    }
}
