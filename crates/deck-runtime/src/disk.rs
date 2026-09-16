//! What each project costs on disk, and where that cost actually sits.
//!
//! # Why this is not `du`
//!
//! Two Windows-specific hazards make a naive recursive sum wrong here, and
//! both are present in Tanner's real workspace:
//!
//! 1. **Reparse points.** There are 28 directory junctions under
//!    `Dev/Python/Soundcloud-Downloader/Radio/`, pointing back into the same
//!    project. Following them counts that tree once per junction, and a
//!    junction aimed at an ancestor would recurse until the stack gave out.
//!    Every entry is therefore stat-ed without traversing (`DirEntry::metadata`
//!    does not follow the link) and skipped if it carries
//!    `FILE_ATTRIBUTE_REPARSE_POINT`.
//!
//!    An earlier version of this comment cited `Dev/JS/CouponHunter` as the
//!    motivating junction. **That path does not exist** -- `dir /AL` on
//!    `Dev/JS` reports none, and the extension is a plain directory at
//!    `Dev/JS/Extensions/Coupon-Hunter`. It was written from memory rather
//!    than from the filesystem, which is exactly the failure this crate's
//!    `facts` module has a header warning about. The hazard is real; the
//!    evidence first given for it was not.
//!
//! 2. **Permission errors mid-walk.** A single unreadable directory must not
//!    void the whole measurement, so unreadable entries are counted as
//!    skipped and reported alongside the total. A number with "3 unreadable"
//!    next to it is honest; a silently-short number is not.
//!
//! # Why the sizes are logical, not on-disk
//!
//! This reports the sum of file lengths, not cluster-rounded allocation size
//! and not compressed size. That is what "this project is 2.1 GB" means to
//! someone deciding whether to delete `node_modules`, and it is the number
//! that stays stable if the folder moves to another volume.

use std::path::{Path, PathBuf};

use serde::Serialize;

/// Depth ceiling for a single walk.
///
/// Nothing legitimate in a source tree nests this far; the cap exists so a
/// pathological structure (or a reparse point the attribute check somehow
/// missed) terminates instead of running until the process is killed.
const MAX_DEPTH: usize = 40;

/// `FILE_ATTRIBUTE_REPARSE_POINT` -- junctions, symlinks and mount points.
#[cfg(windows)]
const REPARSE_POINT: u32 = 0x0400;

/// A measured directory tree.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    /// Sum of file lengths, in bytes.
    pub bytes: u64,
    /// How many files contributed.
    pub files: u64,
    /// Entries that could not be read (permissions, vanished mid-walk) and
    /// links deliberately not followed. Surfaced so a short total can say why.
    pub skipped: u64,
}

/// One immediate child of a directory, with its whole subtree measured.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Child {
    /// File or directory name, no path.
    pub name: String,
    /// Absolute path, so the UI can drill into it without rebuilding it.
    pub path: PathBuf,
    /// True for a directory the caller may expand further.
    pub is_dir: bool,
    /// Measured usage: the file's own length, or the subtree's total.
    pub usage: Usage,
}

/// Whether this entry is a reparse point we must not walk through.
#[cfg(windows)]
fn is_link(meta: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    meta.file_attributes() & REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn is_link(meta: &std::fs::Metadata) -> bool {
    meta.file_type().is_symlink()
}

/// Total usage of everything under `root`, links not followed.
///
/// Iterative rather than recursive: a stack of paths costs one allocation per
/// directory and cannot blow the call stack on a deep tree.
#[must_use]
pub fn usage(root: &Path) -> Usage {
    let mut total = Usage::default();
    let mut stack = vec![(root.to_path_buf(), 0_usize)];

    while let Some((dir, depth)) = stack.pop() {
        if depth > MAX_DEPTH {
            total.skipped += 1;
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            total.skipped += 1;
            continue;
        };
        for entry in entries {
            let Ok(entry) = entry else {
                total.skipped += 1;
                continue;
            };
            // `DirEntry::metadata`, which does NOT traverse a reparse point --
            // unlike `Path::metadata`/`fs::metadata`, which would follow the
            // link and measure the target.
            let Ok(meta) = entry.metadata() else {
                total.skipped += 1;
                continue;
            };
            if is_link(&meta) {
                total.skipped += 1;
            } else if meta.is_dir() {
                stack.push((entry.path(), depth + 1));
            } else {
                total.bytes += meta.len();
                total.files += 1;
            }
        }
    }
    total
}

/// Immediate children of `dir`, each fully measured, largest first.
///
/// The children are measured in parallel because the whole point of this
/// call is a directory like a project root, where one child (`node_modules`,
/// `target`) holds most of the tree and the rest are instant. Sequentially
/// that costs the sum of the walks; in parallel it costs the slowest one.
#[must_use]
pub fn children(dir: &Path) -> Vec<Child> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };

    // Collect first: the walk threads must not hold the directory handle open.
    let mut plan: Vec<(String, PathBuf, bool, u64)> = Vec::new();
    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else { continue };
        let name = entry.file_name().to_string_lossy().into_owned();
        // A link is listed (it is really there and the user can see it in
        // Explorer) but reports zero rather than its target's size, which
        // would be counted again wherever the target actually lives.
        if is_link(&meta) {
            plan.push((name, entry.path(), meta.is_dir(), 0));
        } else if meta.is_dir() {
            plan.push((name, entry.path(), true, 0));
        } else {
            plan.push((name, entry.path(), false, meta.len()));
        }
    }

    let mut out: Vec<Child> = std::thread::scope(|scope| {
        let handles: Vec<_> = plan
            .into_iter()
            .map(|(name, path, is_dir, own_len)| {
                scope.spawn(move || {
                    let measured = if is_dir && !is_link_path(&path) {
                        usage(&path)
                    } else {
                        Usage { bytes: own_len, files: u64::from(!is_dir), skipped: 0 }
                    };
                    Child { name, path, is_dir, usage: measured }
                })
            })
            .collect();
        handles.into_iter().filter_map(|h| h.join().ok()).collect()
    });

    // Largest first: the question this answers is always "what is taking the
    // space", never "what is here alphabetically".
    out.sort_by(|a, b| b.usage.bytes.cmp(&a.usage.bytes).then_with(|| a.name.cmp(&b.name)));
    out
}

/// Re-stats a path to decide whether walking it is safe.
///
/// `children` already knows, but the check is repeated inside the worker
/// because the entry may have been replaced between listing and walking --
/// a real race in a workspace where builds run.
fn is_link_path(path: &Path) -> bool {
    std::fs::symlink_metadata(path).map(|m| is_link(&m)).unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "deck-disk-{tag}-{:x}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn usage_sums_a_nested_tree() {
        let dir = scratch("sum");
        std::fs::write(dir.join("a.txt"), vec![0_u8; 100]).unwrap();
        let sub = dir.join("nested/deeper");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("b.bin"), vec![0_u8; 250]).unwrap();

        let u = usage(&dir);
        assert_eq!(u.bytes, 350);
        assert_eq!(u.files, 2);
        assert_eq!(u.skipped, 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_directory_reports_zero_and_says_it_skipped() {
        let u = usage(Path::new(r"Z:\definitely\not\here"));
        assert_eq!(u.bytes, 0);
        assert_eq!(u.skipped, 1, "an unreadable root must be reported, not silently zero");
    }

    #[test]
    fn children_are_measured_and_sorted_largest_first() {
        let dir = scratch("children");
        std::fs::create_dir_all(dir.join("big")).unwrap();
        std::fs::write(dir.join("big/x"), vec![0_u8; 900]).unwrap();
        std::fs::create_dir_all(dir.join("small")).unwrap();
        std::fs::write(dir.join("small/y"), vec![0_u8; 10]).unwrap();
        std::fs::write(dir.join("loose.txt"), vec![0_u8; 50]).unwrap();

        let kids = children(&dir);
        assert_eq!(kids.len(), 3);
        assert_eq!(kids[0].name, "big");
        assert_eq!(kids[0].usage.bytes, 900);
        assert!(kids[0].is_dir);
        assert_eq!(kids[1].name, "loose.txt");
        assert_eq!(kids[1].usage.bytes, 50);
        assert!(!kids[1].is_dir);
        assert_eq!(kids[2].name, "small");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The `Soundcloud-Downloader/Radio` case: a junction inside a measured
    /// tree must not be followed, or its target is counted once per junction.
    #[cfg(windows)]
    #[test]
    fn a_directory_junction_is_skipped_not_followed() {
        let dir = scratch("junction");
        let real = dir.join("real");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(real.join("payload.bin"), vec![0_u8; 500]).unwrap();

        let link = dir.join("link");
        let made = std::process::Command::new("cmd")
            .args(["/c", "mklink", "/J", &link.display().to_string(), &real.display().to_string()])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if !made {
            let _ = std::fs::remove_dir_all(&dir);
            return; // mklink /J needs no elevation, but do not fail the suite if it is unavailable.
        }

        let u = usage(&dir);
        assert_eq!(u.bytes, 500, "the junction target must be counted exactly once");
        assert!(u.skipped >= 1, "the junction itself should be reported as skipped");

        // And it is still LISTED, reporting zero rather than the target size.
        let kids = children(&dir);
        let linked = kids.iter().find(|c| c.name == "link").expect("the junction is still shown");
        assert_eq!(linked.usage.bytes, 0);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
