//! Cached filesystem access for detection.
//!
//! Every runner is consulted against every candidate directory, and a dozen
//! runners asking "does `package.json` exist?" and "does it have `scripts/dev`?"
//! would otherwise mean a dozen `stat` calls and a dozen JSON parses of the same
//! file. A scan across a workspace multiplies that by the directory count.
//!
//! [`ProbeCache`] is scoped to one directory and memoises three things:
//! existence checks, file contents, and parsed JSON/TOML documents. It is
//! created per directory and dropped when detection for that directory finishes,
//! so nothing goes stale and memory does not accumulate across a scan.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Cap on how much of a file is read for a `file_contains` rule.
///
/// A manifest pointing this at a multi-gigabyte artefact should cost a bounded
/// read, not stall a scan.
const MAX_READ_BYTES: usize = 256 * 1024;

/// The parse state of a structured file.
///
/// `Invalid` is distinct from absent on purpose: a project with a syntactically
/// broken `package.json` should not silently read as "not a Node project", and a
/// rule that needs to parse it can report the real problem.
#[derive(Debug, Clone)]
enum Parsed<T> {
    /// File is absent.
    Missing,
    /// File parsed successfully.
    Ok(T),
    /// File exists but could not be parsed.
    Invalid(String),
}

/// Memoised filesystem and parse results for a single project directory.
///
/// Interior mutability keeps [`crate::rule::evaluate`] taking `&ProbeCache`,
/// which in turn lets `Runner::detect` keep its `&self` receiver. The cache is
/// single-threaded by construction -- one per directory, used by one detection
/// pass -- so a `RefCell` is the honest choice over a lock nobody contends.
pub struct ProbeCache {
    root: PathBuf,
    exists: RefCell<HashMap<String, bool>>,
    is_dir: RefCell<HashMap<String, bool>>,
    text: RefCell<HashMap<String, Option<String>>>,
    json: RefCell<HashMap<String, Parsed<serde_json::Value>>>,
    toml: RefCell<HashMap<String, Parsed<toml::Value>>>,
    entries: RefCell<Option<Vec<String>>>,
}

impl ProbeCache {
    /// Creates a cache rooted at a project directory.
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            exists: RefCell::new(HashMap::new()),
            is_dir: RefCell::new(HashMap::new()),
            text: RefCell::new(HashMap::new()),
            json: RefCell::new(HashMap::new()),
            toml: RefCell::new(HashMap::new()),
            entries: RefCell::new(None),
        }
    }

    /// The directory this cache describes.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Filenames in a directory relative to the root, sorted.
    ///
    /// Uncached on purpose: build-output directories change between a detection
    /// pass and a launch, and a cached "no binary here" would outlive the build
    /// that produced one. The root listing stays cached because it is hit
    /// constantly during detection and rarely changes mid-scan.
    #[must_use]
    pub fn entries_in(&self, dir: &str) -> Vec<String> {
        let Ok(read) = std::fs::read_dir(self.root().join(dir)) else {
            return Vec::new();
        };
        let mut names: Vec<String> = read
            .filter_map(std::result::Result::ok)
            // Directories cannot be launched, and a folder named `x.exe` would
            // otherwise be offered as a binary.
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// Resolves a manifest-relative path, refusing to escape the project root.
    ///
    /// A manifest is user-supplied data, and `../../../Windows/System32` in a
    /// `file_exists` rule must not turn detection into a filesystem probe of the
    /// whole machine. Returns `None` for any path that would leave the root.
    fn resolve(&self, rel: &str) -> Option<PathBuf> {
        use std::path::Component;

        let candidate = Path::new(rel);
        if candidate.is_absolute() {
            return None;
        }

        let mut depth = 0i32;
        for component in candidate.components() {
            match component {
                Component::Normal(_) => depth += 1,
                Component::CurDir => {}
                Component::ParentDir => {
                    depth -= 1;
                    if depth < 0 {
                        return None;
                    }
                }
                // Prefix or RootDir on a supposedly relative path.
                Component::Prefix(_) | Component::RootDir => return None,
            }
        }

        Some(self.root.join(candidate))
    }

    /// Whether a path exists, as a file or a directory.
    pub fn exists(&self, rel: &str) -> bool {
        if let Some(hit) = self.exists.borrow().get(rel) {
            return *hit;
        }
        let value = self
            .resolve(rel)
            .is_some_and(|p| p.try_exists().unwrap_or(false));
        self.exists.borrow_mut().insert(rel.to_owned(), value);
        value
    }

    /// Whether a path exists and is a directory.
    pub fn is_dir(&self, rel: &str) -> bool {
        if let Some(hit) = self.is_dir.borrow().get(rel) {
            return *hit;
        }
        let value = self.resolve(rel).is_some_and(|p| p.is_dir());
        self.is_dir.borrow_mut().insert(rel.to_owned(), value);
        value
    }

    /// File names directly inside the project root, for glob rules.
    ///
    /// Read once per cache. An unreadable directory yields an empty list rather
    /// than an error: a permissions problem on one directory during a scan
    /// should skip it, not abort the scan.
    pub fn root_entries(&self) -> Vec<String> {
        if let Some(cached) = self.entries.borrow().as_ref() {
            return cached.clone();
        }
        let names: Vec<String> = std::fs::read_dir(&self.root)
            .map(|rd| {
                rd.filter_map(std::result::Result::ok)
                    .filter_map(|e| e.file_name().into_string().ok())
                    .collect()
            })
            .unwrap_or_default();
        *self.entries.borrow_mut() = Some(names.clone());
        names
    }

    /// The first [`MAX_READ_BYTES`] of a file as UTF-8, lossily decoded.
    ///
    /// Lossy rather than strict because a `file_contains` rule searching a file
    /// with one invalid byte should still work; the alternative is detection
    /// failing on an encoding technicality.
    pub fn text(&self, rel: &str) -> Option<String> {
        if let Some(hit) = self.text.borrow().get(rel) {
            return hit.clone();
        }
        let value = self.resolve(rel).and_then(|path| {
            std::fs::read(&path).ok().map(|bytes| {
                let end = bytes.len().min(MAX_READ_BYTES);
                String::from_utf8_lossy(&bytes[..end]).into_owned()
            })
        });
        self.text.borrow_mut().insert(rel.to_owned(), value.clone());
        value
    }

    /// Looks up an RFC 6901 pointer in a JSON file.
    ///
    /// A leading `/` is optional so manifests can read `scripts/dev`.
    /// Returns `None` if the file is missing, unparseable, or lacks the key.
    pub fn json_pointer(&self, rel: &str, pointer: &str) -> Option<serde_json::Value> {
        self.with_json(rel, |doc| {
            let normalised = normalise_pointer(pointer);
            doc.pointer(&normalised).cloned()
        })
    }

    /// Looks up a dotted key path in a TOML file.
    pub fn toml_path(&self, rel: &str, path: &str) -> Option<toml::Value> {
        self.with_toml(rel, |doc| {
            let mut cursor = doc;
            for segment in path.split('.').filter(|s| !s.is_empty()) {
                cursor = cursor.get(segment)?;
            }
            Some(cursor.clone())
        })
    }

    fn with_json<T>(
        &self,
        rel: &str,
        f: impl FnOnce(&serde_json::Value) -> Option<T>,
    ) -> Option<T> {
        if !self.json.borrow().contains_key(rel) {
            let parsed = match self.text(rel) {
                None => Parsed::Missing,
                Some(raw) => match serde_json::from_str::<serde_json::Value>(&raw) {
                    Ok(v) => Parsed::Ok(v),
                    Err(e) => Parsed::Invalid(e.to_string()),
                },
            };
            self.json.borrow_mut().insert(rel.to_owned(), parsed);
        }
        match self.json.borrow().get(rel) {
            Some(Parsed::Ok(doc)) => f(doc),
            _ => None,
        }
    }

    fn with_toml<T>(&self, rel: &str, f: impl FnOnce(&toml::Value) -> Option<T>) -> Option<T> {
        if !self.toml.borrow().contains_key(rel) {
            let parsed = match self.text(rel) {
                None => Parsed::Missing,
                // `toml::from_str`, not `raw.parse()`: as of toml 1.x
                // `FromStr for Value` parses a single *value*, so parsing a
                // whole document that way fails on the first table header.
                Some(raw) => match toml::from_str::<toml::Value>(&raw) {
                    Ok(v) => Parsed::Ok(v),
                    Err(e) => Parsed::Invalid(e.to_string()),
                },
            };
            self.toml.borrow_mut().insert(rel.to_owned(), parsed);
        }
        match self.toml.borrow().get(rel) {
            Some(Parsed::Ok(doc)) => f(doc),
            _ => None,
        }
    }

    /// Why a structured file failed to parse, if it did.
    ///
    /// Lets the UI say "your package.json has a trailing comma on line 12"
    /// instead of "unknown project type".
    pub fn parse_error(&self, rel: &str) -> Option<String> {
        if let Some(Parsed::Invalid(msg)) = self.json.borrow().get(rel) {
            return Some(msg.clone());
        }
        if let Some(Parsed::Invalid(msg)) = self.toml.borrow().get(rel) {
            return Some(msg.clone());
        }
        None
    }
}

/// Normalises a readable key path into an RFC 6901 pointer.
fn normalise_pointer(pointer: &str) -> String {
    if pointer.is_empty() || pointer.starts_with('/') {
        pointer.to_owned()
    } else {
        format!("/{pointer}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch directory that cleans itself up.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let mut path = std::env::temp_dir();
            path.push(format!("deck-probe-{tag}-{}", uuid_like()));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn write(&self, name: &str, contents: &str) {
            let target = self.0.join(name);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(target, contents).unwrap();
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn uuid_like() -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        format!("{nanos:x}")
    }

    #[test]
    fn detects_existing_and_missing_files() {
        let dir = TempDir::new("exists");
        dir.write("package.json", "{}");
        let cache = ProbeCache::new(dir.path());
        assert!(cache.exists("package.json"));
        assert!(!cache.exists("Cargo.toml"));
    }

    #[test]
    fn distinguishes_directories_from_files() {
        let dir = TempDir::new("isdir");
        dir.write("src/main.rs", "fn main() {}");
        let cache = ProbeCache::new(dir.path());
        assert!(cache.is_dir("src"));
        assert!(!cache.is_dir("src/main.rs"));
        assert!(cache.exists("src/main.rs"));
    }

    #[test]
    fn reads_json_pointers_with_and_without_leading_slash() {
        let dir = TempDir::new("json");
        dir.write(
            "package.json",
            r#"{"name":"app","version":"1.2.3","scripts":{"dev":"vite"}}"#,
        );
        let cache = ProbeCache::new(dir.path());
        assert_eq!(
            cache.json_pointer("package.json", "scripts/dev").unwrap(),
            serde_json::json!("vite")
        );
        assert_eq!(
            cache.json_pointer("package.json", "/version").unwrap(),
            serde_json::json!("1.2.3")
        );
        assert!(cache.json_pointer("package.json", "scripts/build").is_none());
    }

    #[test]
    fn reads_dotted_toml_paths() {
        let dir = TempDir::new("toml");
        dir.write(
            "pyproject.toml",
            "[tool.poetry]\nname = \"app\"\nversion = \"0.4.0\"\n",
        );
        let cache = ProbeCache::new(dir.path());
        assert!(cache.toml_path("pyproject.toml", "tool.poetry").is_some());
        assert_eq!(
            cache
                .toml_path("pyproject.toml", "tool.poetry.version")
                .unwrap()
                .as_str()
                .unwrap(),
            "0.4.0"
        );
        assert!(cache.toml_path("pyproject.toml", "tool.pdm").is_none());
    }

    #[test]
    fn malformed_json_is_reported_rather_than_read_as_absent() {
        let dir = TempDir::new("badjson");
        dir.write("package.json", r#"{"name": "app",}"#);
        let cache = ProbeCache::new(dir.path());
        assert!(cache.json_pointer("package.json", "name").is_none());
        assert!(cache.parse_error("package.json").is_some());
        // The file itself is still there, which is the distinction that matters.
        assert!(cache.exists("package.json"));
    }

    #[test]
    fn path_traversal_is_refused() {
        let dir = TempDir::new("escape");
        dir.write("inside.txt", "x");
        let cache = ProbeCache::new(dir.path());
        // These all exist on any Windows box; none may be reachable from a rule.
        assert!(!cache.exists("../"));
        assert!(!cache.exists("..\\..\\..\\Windows\\System32\\drivers\\etc\\hosts"));
        assert!(!cache.exists("C:\\Windows\\System32\\config\\SAM"));
        assert!(!cache.exists("/etc/passwd"));
        // A path that dips into a subdirectory and back out stays inside.
        assert!(cache.exists("./inside.txt"));
    }

    #[test]
    fn lists_root_entries_once() {
        let dir = TempDir::new("entries");
        dir.write("App.csproj", "<Project/>");
        dir.write("App.sln", "");
        let cache = ProbeCache::new(dir.path());
        let first = cache.root_entries();
        assert!(first.contains(&"App.csproj".to_owned()));
        assert!(first.contains(&"App.sln".to_owned()));
        // Second call is served from cache and agrees.
        assert_eq!(cache.root_entries(), first);
    }

    #[test]
    fn missing_directory_yields_empty_entries_not_an_error() {
        let cache = ProbeCache::new(Path::new(r"D:\definitely-not-a-real-path-xyz"));
        assert!(cache.root_entries().is_empty());
        assert!(!cache.exists("anything"));
    }

    #[test]
    fn large_files_are_read_up_to_the_cap() {
        let dir = TempDir::new("big");
        let big = "a".repeat(MAX_READ_BYTES + 5_000);
        dir.write("big.txt", &big);
        let cache = ProbeCache::new(dir.path());
        assert_eq!(cache.text("big.txt").unwrap().len(), MAX_READ_BYTES);
    }
}
