//! Runner manifests compiled into the binary.
//!
//! Generated from the `runners/` directory at the workspace root by
//! `xtask/regenerate-builtins`. A fresh install detects every ecosystem listed
//! here with no setup, and a user manifest in the app data directory sharing an
//! id replaces the bundled one -- see [`crate::registry`].
//!
//! Embedding these rather than shipping loose files means a broken install
//! cannot produce an app that detects nothing, and the accompanying registry
//! test proves at build time that all of them parse and validate.

/// Bundled manifests as `(filename, contents)` pairs.
pub const MANIFESTS: &[(&str, &str)] = &[
    ("batch.toml", include_str!("../../../runners/batch.toml")),
    ("bun.toml", include_str!("../../../runners/bun.toml")),
    ("cmake.toml", include_str!("../../../runners/cmake.toml")),
    ("dart.toml", include_str!("../../../runners/dart.toml")),
    ("deno.toml", include_str!("../../../runners/deno.toml")),
    ("dotnet.toml", include_str!("../../../runners/dotnet.toml")),
    ("electron.toml", include_str!("../../../runners/electron.toml")),
    ("executable.toml", include_str!("../../../runners/executable.toml")),
    ("flutter.toml", include_str!("../../../runners/flutter.toml")),
    ("go.toml", include_str!("../../../runners/go.toml")),
    ("java-gradle.toml", include_str!("../../../runners/java-gradle.toml")),
    ("java-maven.toml", include_str!("../../../runners/java-maven.toml")),
    ("laravel.toml", include_str!("../../../runners/laravel.toml")),
    ("lua.toml", include_str!("../../../runners/lua.toml")),
    ("make.toml", include_str!("../../../runners/make.toml")),
    ("nextjs.toml", include_str!("../../../runners/nextjs.toml")),
    ("node.toml", include_str!("../../../runners/node.toml")),
    ("php.toml", include_str!("../../../runners/php.toml")),
    ("powershell.toml", include_str!("../../../runners/powershell.toml")),
    ("python-poetry.toml", include_str!("../../../runners/python-poetry.toml")),
    ("python-uv.toml", include_str!("../../../runners/python-uv.toml")),
    ("python.toml", include_str!("../../../runners/python.toml")),
    ("rails.toml", include_str!("../../../runners/rails.toml")),
    ("react-native.toml", include_str!("../../../runners/react-native.toml")),
    ("ruby.toml", include_str!("../../../runners/ruby.toml")),
    ("rust.toml", include_str!("../../../runners/rust.toml")),
    ("shell.toml", include_str!("../../../runners/shell.toml")),
    ("tauri-built.toml", include_str!("../../../runners/tauri-built.toml")),
    ("tauri.toml", include_str!("../../../runners/tauri.toml")),
    ("vite.toml", include_str!("../../../runners/vite.toml")),
    ("web.toml", include_str!("../../../runners/web.toml")),
    ("zig.toml", include_str!("../../../runners/zig.toml")),
];

#[cfg(test)]
mod tests {
    use super::MANIFESTS;

    #[test]
    fn every_manifest_is_non_empty_and_uniquely_named() {
        assert!(!MANIFESTS.is_empty());
        let mut names: Vec<&str> = MANIFESTS.iter().map(|(n, _)| *n).collect();
        let total = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), total, "duplicate bundled manifest filename");
        for (name, body) in MANIFESTS {
            assert!(!body.trim().is_empty(), "{name} is empty");
        }
    }

    /// Every bundled manifest has a file, and every file is bundled.
    ///
    /// `MANIFESTS` is hand-maintained, so a new `runners/*.toml` can sit there
    /// doing nothing because nobody added the `include_str!` line -- which is
    /// silent, since the app simply fails to detect that ecosystem and looks
    /// like it never supported it. Reading the directory catches both
    /// directions at once.
    #[test]
    fn the_bundle_matches_the_runners_directory() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../runners");
        let mut on_disk: Vec<String> = std::fs::read_dir(&dir)
            .expect("runners directory")
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("toml")))
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        on_disk.sort();

        let mut bundled: Vec<String> = MANIFESTS.iter().map(|(n, _)| (*n).to_owned()).collect();
        bundled.sort();

        assert_eq!(
            bundled, on_disk,
            "runners/ and MANIFESTS disagree -- add or remove the include_str! line"
        );
    }

    /// The count quoted in README.md and docs/ROADMAP.md.
    ///
    /// Documentation cites this number in four places, and it went stale
    /// without anyone noticing when `tauri-built.toml` was added -- the docs
    /// said 31 while 32 shipped, and the error only came to light because
    /// removing one made the wrong number right again. A number in prose has
    /// no way to fail; this does.
    #[test]
    fn the_documented_runner_count_is_still_correct() {
        assert_eq!(
            MANIFESTS.len(),
            32,
            "bundled runner count changed -- update the count in README.md and docs/ROADMAP.md, then this test"
        );
    }
}
