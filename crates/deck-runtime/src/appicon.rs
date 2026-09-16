//! Project icon discovery and extraction.
//!
//! Every project tile wants a real face. This module finds the best icon a
//! project already has and turns it into PNG bytes the UI can show:
//!
//! 1. **Icon files** the project ships — a Tauri `src-tauri/icons/icon.ico`,
//!    a root `icon.ico`/`app.ico`, a web `favicon.ico`, or a plain `icon.png`.
//!    These are read as-is; browsers render `.ico` and `.png` natively.
//! 2. **Built executables** — the icon is extracted from the `.exe` via
//!    `SHDefExtractIconW` and re-encoded as PNG. Candidate exes are searched in
//!    conventional output directories only (root, `target/release`, `dist`,
//!    `build`, `bin`), never recursively; an exe named like the project wins.
//!
//! Discovery is read-only and bounded, in the same spirit as detection: looking
//! at a project must never execute it.

use std::io;
use std::path::{Path, PathBuf};

/// What discovery found, in priority order of source kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IconSource {
    /// An `.ico` file to serve verbatim.
    Ico(PathBuf),
    /// A `.png` file to serve verbatim.
    Png(PathBuf),
    /// An executable whose embedded icon should be extracted.
    Exe(PathBuf),
}

impl IconSource {
    /// The path behind this source.
    #[must_use]
    pub fn path(&self) -> &Path {
        match self {
            Self::Ico(p) | Self::Png(p) | Self::Exe(p) => p,
        }
    }
}

/// Icon files probed relative to the project root, best first.
///
/// Tauri's shipped icon set leads because it is purpose-made and high quality;
/// favicons trail because they are often 16-32px.
const ICON_FILE_CANDIDATES: &[&str] = &[
    "src-tauri/icons/icon.ico",
    "src-tauri/icons/128x128.png",
    // A plain `icons/` directory. Every non-Tauri project in this workspace --
    // Python tools, C++ apps, browser extensions -- keeps its mark here, and
    // omitting it meant eleven projects with perfectly good artwork on disk
    // silently fell back to the runner glyph. Checked before the root-level
    // names because a project that has bothered to make an icon SET has a
    // better answer than a stray `icon.png` beside its source.
    "icons/icon.ico",
    "icons/icon256.png",
    "icons/icon128.png",
    "icon.ico",
    "app.ico",
    "icon.png",
    "assets/icon.ico",
    "assets/icon.png",
    // Web projects: the generated set lands in `public/`, and the PNGs are
    // preferred over favicon.ico because they are the larger, cleaner frames.
    "public/icon-512.png",
    "public/icon-192.png",
    "public/apple-touch-icon.png",
    "public/favicon.ico",
    "favicon.ico",
];

/// Directories searched (non-recursively) for a built executable.
const EXE_DIRS: &[&str] = &[
    ".",
    "target/release",
    "src-tauri/target/release",
    "dist",
    "build",
    "bin",
];

/// Substrings that mark an executable as build tooling rather than the product.
///
/// A build tree is full of these -- JUCE's `juce_vst3_helper`, vcredist
/// bundles, Crashpad handlers, test runners. Borrowing one of their icons would
/// put a stranger's face on the project tile, which is worse than falling back
/// to the runner glyph.
const HELPER_MARKERS: &[&str] = &[
    "helper",
    "install",
    "uninstall",
    "setup",
    "updater",
    "crashpad",
    "crashreport",
    "vcredist",
    "redist",
    "vc_redist",
    "test",
    "bench",
    "example",
    "sample",
    "tool",
    "codesign",
    "symbolupload",
];

/// Whether a filename stem looks like build tooling.
fn is_helper(stem: &str) -> bool {
    let lower = stem.to_ascii_lowercase();
    HELPER_MARKERS.iter().any(|m| lower.contains(m))
}

/// Finds the best icon source for a project directory.
#[must_use]
pub fn find_icon_source(root: &Path) -> Option<IconSource> {
    for candidate in ICON_FILE_CANDIDATES {
        let path = root.join(candidate);
        if path.is_file() {
            let source = if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("png")) {
                IconSource::Png(path)
            } else {
                IconSource::Ico(path)
            };
            return Some(source);
        }
    }

    find_project_exe(root).map(IconSource::Exe)
}

/// The most plausible built executable for a project.
///
/// Within each conventional directory, an exe whose name resembles the project
/// directory's name wins; otherwise the lexicographically first exe does, so
/// the choice is stable across launches.
#[must_use]
pub fn find_project_exe(root: &Path) -> Option<PathBuf> {
    let project_name = root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .replace(['-', '_', ' '], "");

    for dir in EXE_DIRS {
        let dir_path = root.join(dir);
        let Ok(entries) = std::fs::read_dir(&dir_path) else {
            continue;
        };
        let mut exes: Vec<PathBuf> = entries
            .filter_map(std::result::Result::ok)
            .map(|e| e.path())
            .filter(|p| {
                p.is_file() && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe"))
            })
            .collect();
        if exes.is_empty() {
            continue;
        }
        exes.sort();

        let normalised = |p: &Path| {
            p.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase()
                .replace(['-', '_', ' '], "")
        };

        // A name match is trusted even if it trips a helper marker -- a project
        // genuinely called "Test-Runner" owns its own name.
        if let Some(hit) = exes.iter().find(|p| {
            let stem = normalised(p);
            !project_name.is_empty()
                && (stem.contains(&project_name) || project_name.contains(&stem))
        }) {
            return Some(hit.clone());
        }

        // Otherwise take the first exe that is not obviously build tooling.
        // If every candidate in this directory is tooling, fall through to the
        // next directory -- and if none of them yield anything, claim no icon at
        // all, because the runner glyph beats a helper's face on the tile.
        if let Some(hit) = exes.iter().find(|p| !is_helper(&normalised(p))) {
            return Some(hit.clone());
        }
    }

    None
}

/// Loads a source as `(mime, bytes)` ready for a data URL.
///
/// # Errors
///
/// Returns an [`io::Error`] if the file cannot be read or the executable's
/// icon cannot be extracted and encoded.
pub fn load_icon(source: &IconSource) -> io::Result<(&'static str, Vec<u8>)> {
    match source {
        IconSource::Ico(path) => Ok(("image/x-icon", std::fs::read(path)?)),
        IconSource::Png(path) => Ok(("image/png", std::fs::read(path)?)),
        IconSource::Exe(path) => Ok(("image/png", extract_exe_icon_png(path)?)),
    }
}

/// Extracts the largest icon from an executable and encodes it as PNG.
///
/// # Errors
///
/// Returns an [`io::Error`] if the exe has no extractable icon or a Win32
/// call fails.
pub fn extract_exe_icon_png(exe: &Path) -> io::Result<Vec<u8>> {
    let rgba = extract_exe_icon_rgba(exe)?;
    encode_png(&rgba.pixels, rgba.size)
}

struct RgbaIcon {
    /// Square edge length in pixels.
    size: u32,
    /// RGBA, row-major, top-down.
    pixels: Vec<u8>,
}

// Dimension casts below are guarded by the explicit 1..=1024 range check, and
// buffer-size casts are bounded by the same dimensions.
#[allow(
    unsafe_code,
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::items_after_statements,
    clippy::too_many_lines
)]
fn extract_exe_icon_rgba(exe: &Path) -> io::Result<RgbaIcon> {
    use std::os::windows::ffi::OsStrExt;

    use windows::core::PCWSTR;
    use windows::Win32::Graphics::Gdi::{
        DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO,
        BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HGDIOBJ,
    };
    use windows::Win32::UI::Shell::SHDefExtractIconW;
    use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, HICON, ICONINFO};

    let err = |what: &str| io::Error::other(format!("{what} for {}", exe.display()));

    let wide: Vec<u16> = exe.as_os_str().encode_wide().chain(std::iter::once(0)).collect();

    // Ask for the largest classic size the API serves reliably.
    const REQUEST_SIZE: i32 = 256;
    let mut hicon = HICON::default();
    // SAFETY: `wide` is NUL-terminated and outlives the call; `hicon` is a
    // valid out pointer. A non-S_OK result is handled below, not ignored.
    let hr = unsafe {
        SHDefExtractIconW(
            PCWSTR(wide.as_ptr()),
            0,
            0,
            Some(&raw mut hicon),
            None,
            REQUEST_SIZE as u32,
        )
    };
    if hr.is_err() || hicon.is_invalid() {
        return Err(err("no extractable icon"));
    }

    // Guard: the icon and everything derived from it must be released on every
    // exit path from here on.
    struct IconGuard(HICON);
    impl Drop for IconGuard {
        fn drop(&mut self) {
            // SAFETY: handle came from SHDefExtractIconW, destroyed exactly once.
            let _ = unsafe { DestroyIcon(self.0) };
        }
    }
    let _icon_guard = IconGuard(hicon);

    let mut info = ICONINFO::default();
    // SAFETY: valid icon handle, valid out pointer.
    unsafe { GetIconInfo(hicon, &raw mut info) }.map_err(|_| err("GetIconInfo failed"))?;

    struct BitmapGuard(HGDIOBJ);
    impl Drop for BitmapGuard {
        fn drop(&mut self) {
            if !self.0.is_invalid() {
                // SAFETY: bitmap handle owned by us via GetIconInfo docs.
                let _ = unsafe { DeleteObject(self.0) };
            }
        }
    }
    let _color_guard = BitmapGuard(info.hbmColor.into());
    let _mask_guard = BitmapGuard(info.hbmMask.into());

    if info.hbmColor.is_invalid() {
        return Err(err("icon has no colour bitmap"));
    }

    let mut bmp = BITMAP::default();
    // SAFETY: hbmColor is a valid bitmap; the buffer is sized to BITMAP.
    let got = unsafe {
        GetObjectW(
            info.hbmColor.into(),
            std::mem::size_of::<BITMAP>() as i32,
            Some(std::ptr::from_mut(&mut bmp).cast()),
        )
    };
    if got == 0 {
        return Err(err("GetObject failed"));
    }

    let width = bmp.bmWidth;
    let height = bmp.bmHeight;
    if width <= 0 || height <= 0 || width > 1024 || height > 1024 {
        return Err(err("implausible icon dimensions"));
    }

    let mut header = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            // Negative height requests a top-down DIB, matching PNG row order.
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };

    let mut bgra = vec![0u8; (width as usize) * (height as usize) * 4];
    // SAFETY: screen DC for GetDIBits; released below on all paths.
    let dc = unsafe { GetDC(None) };
    // SAFETY: buffer is exactly width*height*4 bytes as the header describes.
    let lines = unsafe {
        GetDIBits(
            dc,
            info.hbmColor,
            0,
            height as u32,
            Some(bgra.as_mut_ptr().cast()),
            &raw mut header,
            DIB_RGB_COLORS,
        )
    };
    // SAFETY: dc came from GetDC(None).
    unsafe { ReleaseDC(None, dc) };
    if lines == 0 {
        return Err(err("GetDIBits failed"));
    }

    // BGRA -> RGBA. Icons drawn without an alpha channel decode as all-zero
    // alpha; treat that as fully opaque rather than rendering nothing.
    let all_transparent = bgra.chunks_exact(4).all(|px| px[3] == 0);
    let mut rgba = Vec::with_capacity(bgra.len());
    for px in bgra.chunks_exact(4) {
        rgba.extend_from_slice(&[px[2], px[1], px[0], if all_transparent { 255 } else { px[3] }]);
    }

    Ok(RgbaIcon {
        size: width as u32,
        pixels: rgba,
    })
}

/// Encodes square RGBA pixels as a PNG.
fn encode_png(rgba: &[u8], size: u32) -> io::Result<Vec<u8>> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, size, size);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|e| io::Error::other(format!("png header: {e}")))?;
        writer
            .write_image_data(rgba)
            .map_err(|e| io::Error::other(format!("png data: {e}")))?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Tree(PathBuf);

    impl Tree {
        fn new(tag: &str) -> Self {
            use std::time::{SystemTime, UNIX_EPOCH};
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let mut path = std::env::temp_dir();
            path.push(format!("deck-icon-{tag}-{nanos:x}"));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn file(&self, rel: &str) -> &Self {
            let target = self.0.join(rel);
            std::fs::create_dir_all(target.parent().unwrap()).unwrap();
            std::fs::write(target, b"x").unwrap();
            self
        }
    }

    impl Drop for Tree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn tauri_icon_beats_everything() {
        let t = Tree::new("tauri");
        t.file("src-tauri/icons/icon.ico")
            .file("icon.ico")
            .file("target/release/app.exe");
        match find_icon_source(&t.0) {
            Some(IconSource::Ico(p)) => assert!(p.ends_with("src-tauri/icons/icon.ico")),
            other => panic!("expected the tauri ico, got {other:?}"),
        }
    }

    #[test]
    fn favicon_is_used_when_nothing_better_exists() {
        let t = Tree::new("favicon");
        t.file("public/favicon.ico");
        assert!(matches!(find_icon_source(&t.0), Some(IconSource::Ico(_))));
    }

    #[test]
    fn png_icons_are_typed_as_png() {
        let t = Tree::new("png");
        t.file("icon.png");
        assert!(matches!(find_icon_source(&t.0), Some(IconSource::Png(_))));
    }

    #[test]
    fn exe_is_the_last_resort() {
        let t = Tree::new("exe");
        t.file("target/release/thing.exe");
        assert!(matches!(find_icon_source(&t.0), Some(IconSource::Exe(_))));
    }

    #[test]
    fn a_name_matching_exe_beats_alphabetical_order() {
        let t = Tree::new("My-App");
        t.file("target/release/aaa-helper.exe")
            .file("target/release/my_app.exe");
        let exe = find_project_exe(&t.0).unwrap();
        assert!(exe.ends_with("my_app.exe"), "got {exe:?}");
    }

    #[test]
    fn build_helpers_are_not_mistaken_for_the_product() {
        // Vesper's build tree contains JUCE's helper; borrowing its icon would
        // put the wrong face on the tile.
        let t = Tree::new("Vesper");
        t.file("build/juce_vst3_helper.exe");
        assert_eq!(
            find_project_exe(&t.0),
            None,
            "a lone build helper must not be adopted as the project exe"
        );
    }

    #[test]
    fn a_real_product_exe_beats_a_helper_beside_it() {
        let t = Tree::new("Thing");
        t.file("build/juce_vst3_helper.exe").file("build/aaa-product.exe");
        let exe = find_project_exe(&t.0).unwrap();
        assert!(exe.ends_with("aaa-product.exe"), "got {exe:?}");
    }

    #[test]
    fn a_project_named_like_a_helper_still_matches_itself() {
        // "Test" is a helper marker, but a project actually called Test-Runner
        // owns that name -- the name match must win.
        let t = Tree::new("Test-Runner");
        t.file("target/release/test_runner.exe");
        let exe = find_project_exe(&t.0).unwrap();
        assert!(exe.ends_with("test_runner.exe"), "got {exe:?}");
    }

    #[test]
    fn no_candidates_means_none() {
        let t = Tree::new("empty");
        t.file("README.md");
        assert_eq!(find_icon_source(&t.0), None);
    }

    #[test]
    fn exe_search_is_not_recursive() {
        let t = Tree::new("deep");
        t.file("some/deep/nested/dir/thing.exe");
        assert_eq!(find_project_exe(&t.0), None);
    }

    #[test]
    fn extracts_a_real_icon_from_explorer() {
        // explorer.exe always exists and always carries an icon set.
        let windir = std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into());
        let exe = Path::new(&windir).join("explorer.exe");
        let png = extract_exe_icon_png(&exe).expect("explorer icon should extract");
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n", "output is not a PNG");
        assert!(png.len() > 500, "suspiciously small icon: {} bytes", png.len());
    }

    #[test]
    fn a_non_executable_yields_an_error_not_a_panic() {
        let t = Tree::new("notexe");
        t.file("fake.exe"); // one byte of junk
        assert!(extract_exe_icon_png(&t.0.join("fake.exe")).is_err());
    }
}

#[cfg(test)]
mod icon_directory_lookup {
    //! Covers the search paths, because a missing candidate fails SILENTLY:
    //! the project falls back to the runner glyph and looks like a design
    //! choice rather than a lookup that never happened. Eleven projects with
    //! real artwork on disk were invisible this way.

    use super::*;
    use std::fs;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("deck-icon-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    #[test]
    fn a_plain_icons_directory_is_found() {
        let root = scratch("plain");
        fs::create_dir_all(root.join("icons")).unwrap();
        fs::write(root.join("icons/icon.ico"), b"x").unwrap();

        match find_icon_source(&root) {
            // `ends_with` on a Path compares whole components, so the
            // separator style does not matter here.
            Some(IconSource::Ico(p)) => assert!(p.ends_with("icons/icon.ico")),
            other => panic!("expected the icons/ ico, got {other:?}"),
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_generated_png_set_is_found_without_an_ico() {
        // Web and extension layouts ship PNGs only.
        let root = scratch("pngs");
        fs::create_dir_all(root.join("icons")).unwrap();
        fs::write(root.join("icons/icon256.png"), b"x").unwrap();

        assert!(
            matches!(find_icon_source(&root), Some(IconSource::Png(_))),
            "a PNG-only icon set must still resolve"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_public_directory_png_beats_a_bare_favicon() {
        // The generated 512 is a cleaner frame than a 16px favicon.
        let root = scratch("public");
        fs::create_dir_all(root.join("public")).unwrap();
        fs::write(root.join("public/icon-512.png"), b"x").unwrap();
        fs::write(root.join("public/favicon.ico"), b"x").unwrap();

        assert!(
            matches!(find_icon_source(&root), Some(IconSource::Png(_))),
            "the larger PNG should win over favicon.ico"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_tauri_icon_set_still_wins_over_a_plain_icons_dir() {
        // Order matters: a Tauri project's own set is the authoritative one.
        let root = scratch("tauri");
        fs::create_dir_all(root.join("src-tauri/icons")).unwrap();
        fs::create_dir_all(root.join("icons")).unwrap();
        fs::write(root.join("src-tauri/icons/icon.ico"), b"x").unwrap();
        fs::write(root.join("icons/icon.ico"), b"x").unwrap();

        match find_icon_source(&root) {
            Some(IconSource::Ico(p)) => assert!(
                p.to_string_lossy().contains("src-tauri"),
                "the Tauri set must take precedence, got {p:?}"
            ),
            other => panic!("expected an ico, got {other:?}"),
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_project_with_nothing_reports_nothing() {
        // Must stay None rather than inventing a source: the UI's runner-glyph
        // fallback is the correct answer for a project with no artwork.
        let root = scratch("empty");
        assert!(find_icon_source(&root).is_none());
        let _ = fs::remove_dir_all(&root);
    }
}
