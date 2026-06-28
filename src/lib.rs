use std::fs;
use std::env;
use std::path::Path;
use std::path::PathBuf;
use std::io::Result;

/// A single symlink encountered while resolving a path, split into its four
/// meaningful parts.
///
/// For a path like `dir/link/suffix` where `dir/link` is a symlink pointing at
/// `target`:
///
/// * `dirname`  — the directory the symlink lives in (`dir`); the base for
///   resolving a relative `target`.
/// * `basename` — the symlink's own name (`link`); the part being replaced.
/// * `target`   — the raw symlink contents (what `readlink` returns).
/// * `suffix`   — the not-yet-explored remainder of the path (`suffix`).
#[derive(Debug, PartialEq, Clone)]
pub struct Symlink {
    pub dirname: PathBuf,
    pub basename: PathBuf,
    pub target: PathBuf,
    pub suffix: PathBuf,
}

/// The end of a resolution: a path that is not a symlink. `exists` records
/// whether it actually exists on the filesystem (a dangling link resolves to a
/// `NotLink` that does not exist).
#[derive(Debug, PartialEq, Clone)]
pub struct NotLink {
    pub path: PathBuf,
    pub exists: bool,
}

/// The result of resolving a path: a (possibly empty) chain of symlinks
/// followed by exactly one terminal [`NotLink`]. This structure encodes the
/// invariant that resolution always ends in a single non-symlink.
#[derive(Debug, PartialEq, Clone)]
pub struct Resolution {
    pub links: Vec<Symlink>,
    pub end: NotLink,
}

impl Symlink {
    /// The full path of the symlink itself (`dirname/basename`).
    pub fn source(&self) -> PathBuf {
        self.dirname.join(&self.basename)
    }

    /// The path obtained by following this symlink and re-attaching the suffix.
    fn resolve(&self) -> PathBuf {
        let mut base = self.dirname.clone();
        base.push(&self.target);
        let mut resolved = normalize_path(base);
        if self.suffix.iter().next().is_some() {
            resolved.push(&self.suffix);
        }
        resolved
    }
}

/// Abstraction over the filesystem operations `readlinks` needs.
///
/// This lets the resolution logic run against either the real filesystem
/// ([`RealFs`]) or an in-memory mock in tests.
pub trait Filesystem {
    /// Inspect a single path (not following symlinks).
    ///
    /// * `Ok(Some(target))` — `path` is a symlink pointing at `target`.
    /// * `Ok(None)` — `path` exists but is not a symlink.
    /// * `Err(NotFound)` — `path` does not exist.
    fn read_link(&self, path: &Path) -> Result<Option<PathBuf>>;
}

/// The real, on-disk filesystem.
pub struct RealFs;

impl Filesystem for RealFs {
    fn read_link(&self, path: &Path) -> Result<Option<PathBuf>> {
        let metadata = fs::symlink_metadata(path)?;
        if !metadata.file_type().is_symlink() {
            Ok(None)
        } else {
            Ok(Some(fs::read_link(path)?))
        }
    }
}

fn normalize_path(path: PathBuf) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(out.components().next_back(), Some(Component::Normal(_))) {
                    out.pop();
                } else if !matches!(out.components().next_back(), Some(Component::RootDir) | Some(Component::Prefix(_))) {
                    out.push("..");
                }
            }
            _ => out.push(c),
        }
    }
    out
}


// # Resolution

/// One step of resolution: either the first symlink found, or the terminal
/// non-symlink path.
#[derive(Debug, PartialEq)]
enum Step {
    Link(Symlink),
    End(PathBuf),
}

/// Find the first symlink in a `path`.
///
/// Walks the path one component at a time, querying `fs` for each prefix, and
/// returns as soon as a prefix turns out to be a symlink. The remaining,
/// not-yet-explored components are returned as the `suffix`.
fn find_symlink(fs: &dyn Filesystem, path: &Path) -> Result<Step> {
    let mut prefix = PathBuf::new();
    let mut parts = path.components();
    while let Some(part) = parts.next() {
        prefix.push(&part);
        if let Some(target) = fs.read_link(prefix.as_path())? {
            let dirname = prefix.parent().unwrap_or(Path::new("")).to_path_buf();
            let basename = prefix.file_name().map(PathBuf::from).unwrap_or_default();
            return Ok(Step::Link(Symlink {
                dirname,
                basename,
                target,
                suffix: parts.as_path().into(),
            }));
        }
    }
    // Every component existed and none was a symlink: the path exists as-is.
    Ok(Step::End(prefix))
}

/// Resolve `path` against the real filesystem.
pub fn resolve<P: AsRef<Path>>(path: P) -> Resolution {
    resolve_with(RealFs, path)
}

/// Like [`resolve`], but against an arbitrary [`Filesystem`] (used in tests).
///
/// Iteratively follows the first symlink in the path until reaching a
/// non-symlink, collecting every symlink along the way.
pub fn resolve_with<F: Filesystem, P: AsRef<Path>>(fs: F, path: P) -> Resolution {
    let mut current = path.as_ref().to_path_buf();
    let mut links = Vec::new();
    loop {
        match find_symlink(&fs, &current) {
            Ok(Step::Link(link)) => {
                current = link.resolve();
                links.push(link);
            }
            Ok(Step::End(path)) => {
                return Resolution { links, end: NotLink { path, exists: true } };
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Resolution { links, end: NotLink { path: current, exists: false } };
            }
            Err(e) => {
                eprintln!("Accessing {} failed: {}", current.display(), e);
                return Resolution { links, end: NotLink { path: current, exists: false } };
            }
        }
    }
}


// # Layout and formatting
//
// Rendering proceeds in two passes after resolution: `layout` derives the
// display pieces for each row and computes the left offsets that align every
// separator into one column, then `render` assembles those rows into strings.

const RED: &str = "\x1B[41m";
const GREEN: &str = "\x1B[42m";
const RESET: &str = "\x1B[0m";

/// One output row after layout: the display pieces plus the left `offset`
/// (number of leading spaces) that aligns the separators across the chain.
pub struct LayoutRow {
    /// Leading spaces so that all separators land in the same column.
    offset: usize,
    /// The text left of the separator (the resolution so far).
    prefix: String,
    /// The unexplored suffix shown right of the separator, if any.
    suffix: Option<String>,
    /// Whether to tag the row with ` (not found)`.
    missing: bool,
    /// The displayed dirname of a symlink source (`./dir`, `/abs/dir`, or `.` for
    /// a top-level link), used as the base for the verbose debug line.
    dirname: String,
    /// For verbose mode: the raw symlink target shown verbatim on the debug line.
    target: Option<String>,
    /// Whether `target` is absolute (debug line's green is the root `/`, indented
    /// to sit under the normal line's green, rather than the dirname boundary).
    target_abs: bool,
    /// For verbose mode: the byte position in `prefix` of the dirname boundary —
    /// colored green on the normal line, and the column the debug line's green is
    /// aligned to. `None` for the terminal row.
    green: Option<usize>,
}

fn str_of(path: PathBuf) -> String {
    normalize_path(path).display().to_string()
}

/// Prefix a relative path string with `./` so every printed path carries an
/// explicit anchor (an absolute path already starts with `/` and is left as-is).
/// This guarantees the dirname boundary always lands on a real `/`: a top-level
/// link whose dirname is empty becomes `.`/`<name>`, so the boundary is the `/`
/// of the `./` anchor.
fn dotted(s: &str) -> String {
    if s.starts_with('/') {
        s.to_string()
    } else {
        format!("./{}", s)
    }
}

/// Pass 2: turn a [`Resolution`] into laid-out rows with aligned offsets.
///
/// Each path is split at a separator: the part before is the resolution so far,
/// the part after is the not-yet-explored suffix. Relative paths are shown with a
/// leading `./` anchor. In `verbose` mode the symlink source is shown as
/// `dirname/basename` with the dirname boundary recorded for the green mark, and
/// each symlink carries a debug line showing its target. The separators are
/// aligned by giving every row a left offset relative to the longest prefix.
pub fn layout(resolution: &Resolution, verbose: bool) -> Vec<LayoutRow> {
    let mut last_suffix: Option<String> = None;
    let mut rows: Vec<LayoutRow> = Vec::new();

    for link in &resolution.links {
        let suffix = {
            let s = link.suffix.display().to_string();
            if s.is_empty() { None } else { Some(s) }
        };
        last_suffix = suffix.clone();

        let raw_dirname = str_of(link.dirname.clone());
        let basename = link.basename.display().to_string();

        // The displayed dirname carries the `./` anchor for a relative source; an
        // empty dirname (top-level link) collapses to `.`, so the boundary `/` is
        // the one in the `./` anchor.
        let dirname = if raw_dirname.is_empty() {
            ".".to_string()
        } else {
            dotted(&raw_dirname)
        };

        // The prefix (left of the suffix) is the symlink source. In verbose mode
        // it is shown as `dirname/basename` with the dirname boundary marked
        // green; otherwise it is the plain `./source`.
        let (prefix, green) = if verbose {
            let prefix = format!("{}/{}", dirname, basename);
            (prefix, Some(dirname.len()))
        } else {
            (dotted(&str_of(link.source())), None)
        };

        let (target, target_abs) = if verbose {
            (Some(link.target.display().to_string()), link.target.is_absolute())
        } else {
            (None, false)
        };

        rows.push(LayoutRow { offset: 0, prefix, suffix, missing: false, dirname, target, target_abs, green });
    }

    // The terminal NotLink.
    {
        let full = dotted(&str_of(resolution.end.path.clone()));
        // The final resolved path still carries the last suffix as its tail;
        // split it back out so the separator lines up with the rest of the chain.
        let (prefix, suffix) = match &last_suffix {
            Some(s) if full.ends_with(&format!("/{}", s)) => {
                (full[..full.len() - s.len() - 1].to_string(), Some(s.clone()))
            }
            _ => (full, None),
        };
        rows.push(LayoutRow {
            offset: 0,
            prefix,
            suffix,
            missing: !resolution.end.exists,
            dirname: String::new(),
            target: None,
            target_abs: false,
            green: None,
        });
    }

    // Offset pass: align every separator to the column of the longest prefix.
    let width = rows.iter().map(|r| r.prefix.len()).max().unwrap_or(0);
    for r in &mut rows {
        r.offset = width - r.prefix.len();
    }
    rows
}

/// Pass 3: assemble laid-out rows into printable lines.
///
/// The prefix and suffix are joined with a real `/` so each line is a valid,
/// copy-pasteable path. When `color` is set (i.e. stdout is a terminal) the
/// separators are colored; the escape codes are zero-width, so alignment is
/// preserved and a copied line still yields a clean path. Two columns of
/// separators are highlighted:
///
/// * **red** — the separator between the resolved-so-far prefix and the
///   unexplored suffix (the symlink about to be resolved). It appears on every
///   normal line, and on every debug line at the target|suffix boundary. All red
///   separators on normal lines are aligned into one column.
/// * **green** — only in verbose mode, marking the "splice point" where this
///   link's target begins. On a normal line and its relative debug line it is the
///   dirname boundary (the `/` ending the directory the symlink lives in), shown
///   at the same column on both, so the verbatim symlink value is exactly the
///   text between green and red on the debug line. An absolute target ignores the
///   dirname, so its green is the leading root `/`, and the debug line is indented
///   to put that root `/` under the normal line's green. Either way, every debug
///   line's green aligns with its normal line's green, so the greens form their
///   own aligned column just as the reds do.
///
/// The separator is the `/` before the suffix; its background is colored. When
/// there is no suffix the line ends in a single terminator space and the red mark
/// falls on it (a colored block), so the red mark is always present on normal and
/// debug lines alike. When there is a suffix the line has no terminator space and
/// the suffix overhangs to the right of the red `/`; on debug lines that overhang
/// is not part of the alignment shared by the normal lines.
pub fn render(rows: &[LayoutRow], color: bool) -> Vec<String> {
    let mut out = Vec::new();

    for r in rows {
        let real = match &r.suffix {
            Some(s) => format!("{}/{}", r.prefix, s),
            None    => r.prefix.clone(),
        };

        // The normal line: red at the source|suffix boundary (`real.len()` is the
        // terminator space when there is no suffix), plus, in verbose mode, green
        // at the dirname boundary.
        let red = r.prefix.len();
        let mut marks: Vec<(usize, &str)> = vec![(red, RED)];
        if let Some(g) = r.green {
            if g != red { marks.push((g, GREEN)); }
        }

        let pad = " ".repeat(r.offset);
        let tag = if r.missing { " (not found)" } else { "" };
        out.push(format!("{}{}{}", pad, paint(&real, &marks, color, true), tag));

        // Verbose debug line: the symlink target with the unexplored suffix
        // attached, so it reads as a full path. Every debug line carries the same
        // two marks as a normal line — green at the splice point, red at the
        // target|suffix boundary — and its green aligns with its normal line's
        // green, so the green marks form an aligned column just like the red ones.
        // A relative target is shown as `dirname/target` (green at the dirname
        // boundary). An absolute target ignores the dirname, so its green is the
        // leading root `/`, indented to sit under the normal line's green; the
        // verbatim target then runs from green to red. When there is no suffix the
        // red lands on the trailing terminator space, just as on a normal line, so
        // the red mark is never missing; when there is a suffix the line has no
        // terminator and the suffix overhangs to the right of the red `/`.
        if let Some(t) = &r.target {
            // `base` is where this line's body begins (the `dirname/` it shares
            // with its normal line, or the absolute target's root `/`); `green` is
            // the splice point within it.
            let (base, green) = if r.target_abs {
                (t.clone(), 0)
            } else {
                (format!("{}/{}", r.dirname, t), r.green.unwrap_or(0))
            };
            let mut marks: Vec<(usize, &str)> = vec![(green, GREEN)];
            let (body, terminator) = match &r.suffix {
                Some(s) => { marks.push((base.len(), RED)); (format!("{}/{}", base, s), false) }
                None    => { marks.push((base.len(), RED)); (base.clone(), true) }
            };
            // An absolute line aligns its green (root `/`) under the normal line's
            // green; a relative line shares the normal line's offset.
            let indent = if r.target_abs { r.offset + r.green.unwrap_or(0) } else { r.offset };
            out.push(format!("{}{}", " ".repeat(indent), paint(&body, &marks, color, terminator)));
        }
    }
    out
}

/// Render `text`, optionally followed by a single space terminator, applying a
/// background color to the separator characters at the given `marks` (each a byte
/// position and a color escape). A position may point at a `/` in the path or at
/// the trailing terminator space (where the mark shows as a colored block).
fn paint(text: &str, marks: &[(usize, &str)], color: bool, terminator: bool) -> String {
    let line = if terminator { format!("{} ", text) } else { text.to_string() };
    if !color {
        return line;
    }

    let mut marks = marks.to_vec();
    marks.sort_by_key(|m| m.0);

    let mut out = String::new();
    let mut last = 0;
    for (at, col) in marks {
        let ch = line[at..].chars().next().unwrap(); // a '/' or the terminator space
        out.push_str(&line[last..at]);
        out.push_str(col);
        out.push(ch);
        out.push_str(RESET);
        last = at + ch.len_utf8();
    }
    out.push_str(&line[last..]);
    out
}


/// Lookup the full path of a command available in $PATH, or return the input as-is.
pub fn expand_path<P: AsRef<Path>>(path: P) -> PathBuf {
    let path = path.as_ref();
    Some(path)
    // Expand with PATH only if it is a single path component
    .filter(|path| path.components().take(2).count() == 1)
    .and_then(|exe|
        env::var_os("PATH").and_then(|ref paths|
            env::split_paths(paths)
            .map(|prefix| prefix.join(exe))
            .find(|bin| bin.is_file()) // TODO: Check that it is actually executable.
        ))
    .unwrap_or_else(|| path.to_path_buf())
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::io::{Error, ErrorKind};

    /// An in-memory filesystem for tests.
    ///
    /// Each registered path is either a symlink (with a target) or a plain
    /// existing entry (file or directory). Querying an unregistered path yields
    /// `NotFound`, just like the real filesystem.
    #[derive(Clone)]
    enum Entry {
        Link(PathBuf),
        Exists,
    }

    struct MockFs {
        entries: HashMap<PathBuf, Entry>,
    }

    impl MockFs {
        fn new() -> Self {
            MockFs { entries: HashMap::new() }
        }

        /// Register every ancestor of `path` as a plain existing directory,
        /// without clobbering an entry that is already there (e.g. a symlink).
        fn ensure_ancestors(&mut self, path: &Path) {
            for anc in path.ancestors().skip(1) {
                if anc.as_os_str().is_empty() {
                    continue;
                }
                self.entries.entry(anc.to_path_buf()).or_insert(Entry::Exists);
            }
        }

        /// Add a symlink `from -> to`.
        fn symlink(mut self, from: &str, to: &str) -> Self {
            let p = PathBuf::from(from);
            self.ensure_ancestors(&p);
            self.entries.insert(p, Entry::Link(PathBuf::from(to)));
            self
        }

        /// Add a plain file (and its parent directories).
        fn file(mut self, path: &str) -> Self {
            let p = PathBuf::from(path);
            self.ensure_ancestors(&p);
            self.entries.entry(p).or_insert(Entry::Exists);
            self
        }
    }

    impl Filesystem for MockFs {
        fn read_link(&self, path: &Path) -> Result<Option<PathBuf>> {
            match self.entries.get(path) {
                Some(Entry::Link(target)) => Ok(Some(target.clone())),
                Some(Entry::Exists) => Ok(None),
                None => Err(Error::new(ErrorKind::NotFound, "no such file")),
            }
        }
    }

    /// Resolve `start` against `fs` and return the path examined at each step
    /// (source + suffix per link, then the terminal path). This is exactly what
    /// the tool prints, before alignment.
    fn lines(fs: MockFs, start: &str) -> Vec<String> {
        let res = resolve_with(fs, start);
        let mut out: Vec<String> = res.links.iter()
            .map(|link| {
                let mut p = link.source();
                if link.suffix.iter().next().is_some() {
                    p.push(&link.suffix);
                }
                p.display().to_string()
            })
            .collect();
        out.push(res.end.path.display().to_string());
        out
    }

    /// Resolve `start` and render it the way the binary does (all three passes,
    /// without color so the separators are plain `/`).
    fn rendered(fs: MockFs, start: &str, verbose: bool) -> Vec<String> {
        let resolution = resolve_with(fs, start);
        render(&layout(&resolution, verbose), false)
    }

    fn link(dirname: &str, basename: &str, target: &str, suffix: &str) -> Symlink {
        Symlink {
            dirname: dirname.into(),
            basename: basename.into(),
            target: target.into(),
            suffix: suffix.into(),
        }
    }

    // --- normalize_path -------------------------------------------------

    #[test]
    fn normalize_drops_curdir() {
        assert_eq!(normalize_path("a/./b".into()), PathBuf::from("a/b"));
    }

    #[test]
    fn normalize_collapses_parentdir() {
        assert_eq!(normalize_path("a/b/../c".into()), PathBuf::from("a/c"));
    }

    #[test]
    fn normalize_keeps_leading_parentdir() {
        assert_eq!(normalize_path("a/../../b".into()), PathBuf::from("../b"));
    }

    #[test]
    fn normalize_cannot_escape_root() {
        assert_eq!(normalize_path("/a/../../b".into()), PathBuf::from("/b"));
    }

    // --- find_symlink ---------------------------------------------------

    #[test]
    fn find_symlink_splits_into_four_parts() {
        let fs = MockFs::new().symlink("a/b", "x").file("a/b");
        assert_eq!(
            find_symlink(&fs, Path::new("a/b/c/d")).unwrap(),
            Step::Link(link("a", "b", "x", "c/d")),
        );
    }

    #[test]
    fn find_symlink_end_when_no_link() {
        let fs = MockFs::new().file("a/b/c");
        assert_eq!(
            find_symlink(&fs, Path::new("a/b/c")).unwrap(),
            Step::End(PathBuf::from("a/b/c")),
        );
    }

    // --- resolution structure -------------------------------------------

    #[test]
    fn resolution_is_links_then_terminal() {
        let fs = MockFs::new()
            .symlink("a", "b")
            .symlink("b", "c")
            .file("c");
        let res = resolve_with(fs, "a");
        assert_eq!(res.links, vec![
            link("", "a", "b", ""),
            link("", "b", "c", ""),
        ]);
        assert_eq!(res.end, NotLink { path: "c".into(), exists: true });
    }

    #[test]
    fn dangling_link_ends_in_missing_path() {
        let fs = MockFs::new().symlink("link", "nope");
        let res = resolve_with(fs, "link");
        assert_eq!(res.links, vec![link("", "link", "nope", "")]);
        assert_eq!(res.end, NotLink { path: "nope".into(), exists: false });
    }

    // --- full resolution chains ----------------------------------------

    #[test]
    fn resolves_simple_relative_link() {
        let fs = MockFs::new().symlink("link", "target").file("target");
        assert_eq!(lines(fs, "link"), vec!["link", "target"]);
    }

    #[test]
    fn carries_suffix_across_multi_component_target() {
        let fs = MockFs::new()
            .symlink("link", "pkg/share")
            .file("pkg/share/terminfo");
        assert_eq!(
            lines(fs, "link/terminfo"),
            vec!["link/terminfo", "pkg/share/terminfo"],
        );
    }

    #[test]
    fn resolves_sibling_link() {
        let fs = MockFs::new().symlink("dir/a", "b").file("dir/b");
        assert_eq!(lines(fs, "dir/a"), vec!["dir/a", "dir/b"]);
    }

    #[test]
    fn suffix_appears_mid_resolution() {
        // Start with no suffix; a multi-component target introduces one when its
        // first component turns out to be a symlink.
        let fs = MockFs::new()
            .symlink("link", "mid/tail")
            .symlink("mid", "real")
            .file("real/tail");
        assert_eq!(
            lines(fs, "link"),
            vec!["link", "mid/tail", "real/tail"],
        );
    }

    #[test]
    fn suffix_shrinks_mid_chain() {
        // The suffix changes from `p/q` to `q` as resolution digs deeper.
        // `d/p -> e` is relative, so it resolves against `d`, yielding `d/e`.
        let fs = MockFs::new()
            .symlink("link", "d")
            .symlink("d/p", "e")
            .file("d/e/q");
        assert_eq!(
            lines(fs, "link/p/q"),
            vec!["link/p/q", "d/p/q", "d/e/q"],
        );
    }

    #[test]
    fn resolves_absolute_link() {
        let fs = MockFs::new()
            .symlink("/a/link", "/x/y")
            .file("/x/y/z");
        assert_eq!(
            lines(fs, "/a/link/z"),
            vec!["/a/link/z", "/x/y/z"],
        );
    }

    #[test]
    fn resolves_parentdir_in_target() {
        let fs = MockFs::new()
            .symlink("dir/link", "../other/file")
            .file("other/file");
        assert_eq!(
            lines(fs, "dir/link"),
            vec!["dir/link", "other/file"],
        );
    }

    #[test]
    fn resolves_long_mixed_chain() {
        // relative -> relative-multi -> relative-with-.. -> absolute -> file
        let fs = MockFs::new()
            .symlink("terminfo", ".hidden")
            .symlink(".hidden", "subdir/profile")
            .symlink("subdir/profile", "../store/pkg")
            .file("store/pkg/share/x");
        assert_eq!(
            lines(fs, "terminfo/share/x"),
            vec![
                "terminfo/share/x",
                ".hidden/share/x",
                "subdir/profile/share/x",
                "store/pkg/share/x",
            ],
        );
    }

    // --- render: alignment ----------------------------------------------

    #[test]
    fn render_aligns_separator_with_suffix() {
        let fs = MockFs::new().symlink("link", "pkg/share").file("pkg/share/x");
        assert_eq!(
            rendered(fs, "link/x", false),
            vec![
                "     ./link/x ",
                "./pkg/share/x ",
            ],
        );
    }

    #[test]
    fn render_appends_space_when_no_suffix() {
        let fs = MockFs::new().symlink("a", "bb").file("bb");
        assert_eq!(
            rendered(fs, "a", false),
            vec![
                " ./a ",
                "./bb ",
            ],
        );
    }

    #[test]
    fn render_tags_missing_target() {
        let fs = MockFs::new().symlink("link", "nope");
        assert_eq!(
            rendered(fs, "link", false),
            vec![
                "./link ",
                "./nope  (not found)",
            ],
        );
    }

    #[test]
    fn render_color_wraps_separator_in_red() {
        let fs = MockFs::new().symlink("link", "pkg/share").file("pkg/share/x");
        let res = resolve_with(fs, "link/x");
        assert_eq!(
            render(&layout(&res, false), true),
            vec![
                "     ./link\x1B[41m/\x1B[0mx ",
                "./pkg/share\x1B[41m/\x1B[0mx ",
            ],
        );
    }

    #[test]
    fn render_color_marks_only_the_current_separator() {
        // Only the current (red) separator is colored; the carried suffix slash
        // is left plain even when it differs from the current separator.
        let red = "\x1B[41m/\x1B[0m";
        let fs = MockFs::new()
            .symlink("t", "a/b")
            .symlink("a", "aa")
            .file("aa/b/s");
        let res = resolve_with(fs, "t/s");
        let lines = render(&layout(&res, false), true);
        assert_eq!(
            lines,
            vec![
                format!(" ./t{}s ", red),
                format!(" ./a{}b/s ", red),  // carried `s` slash stays plain
                format!("./aa{}b/s ", red),
            ],
        );
        assert!(lines.iter().all(|l| !l.contains("\x1B[34m")), "no blue anywhere");
    }

    #[test]
    fn render_color_terminator_is_red_when_no_suffix() {
        // No suffix on either line, so the red separator sits on the trailing
        // terminator space (shown as a red background block).
        let red_sp = "\x1B[41m \x1B[0m";
        let fs = MockFs::new().symlink("a", "bb").file("bb");
        let res = resolve_with(fs, "a");
        assert_eq!(
            render(&layout(&res, false), true),
            vec![
                format!(" ./a{}", red_sp),
                format!("./bb{}", red_sp),
            ],
        );
    }

    // --- render: verbose ------------------------------------------------

    #[test]
    fn render_verbose_splits_with_slash_and_explains_relative() {
        let fs = MockFs::new().symlink("dir/link", "../other").file("other");
        assert_eq!(
            rendered(fs, "dir/link", true),
            vec![
                "./dir/link ",
                "./dir/../other ",
                "   ./other ",
            ],
        );
    }

    #[test]
    fn render_verbose_attaches_suffix_to_relative_explanation() {
        // The unexplored suffix (`x`) is attached to the explanation line so it
        // reads as a full path, but it overhangs to the right rather than being
        // aligned: the `dir/../other` part keeps its position and `/x` trails.
        let fs = MockFs::new().symlink("dir/link", "../other").file("other/x");
        assert_eq!(
            rendered(fs, "dir/link/x", true),
            vec![
                "./dir/link/x ",
                "./dir/../other/x",
                "   ./other/x ",
            ],
        );
    }

    #[test]
    fn render_verbose_attaches_suffix_to_absolute_explanation() {
        // The absolute debug line's root `/` aligns under the normal line's green
        // (the dirname boundary after `/a`), so it is indented 2 columns; the
        // suffix `/z` overhangs to the right.
        let fs = MockFs::new().symlink("/a/link", "/x/y").file("/x/y/z");
        assert_eq!(
            rendered(fs, "/a/link/z", true),
            vec![
                "/a/link/z ",
                "  /x/y/z",
                "   /x/y/z ",
            ],
        );
    }

    #[test]
    fn render_verbose_aligns_absolute_target_green_under_dirname() {
        // With no suffix the absolute debug line still carries its green root `/`
        // (indented 2 columns, under the normal line's green) and a red terminator
        // space at the end, so it keeps both marks like every other debug line.
        let fs = MockFs::new().symlink("/a/link", "/x/y").file("/x/y");
        assert_eq!(
            rendered(fs, "/a/link", true),
            vec![
                "/a/link ",
                "  /x/y ",
                "   /x/y ",
            ],
        );
    }

    #[test]
    fn render_verbose_color_dirname_is_green_shared_by_both_lines() {
        // Green marks the dirname boundary at the same column on the normal line
        // (`./dir/link`) and the debug line (`./dir/../other`); red marks the
        // suffix boundary. With no suffix the red lands on the trailing terminator
        // space on both lines, so the debug line keeps its red mark too.
        let red_sp = "\x1B[41m \x1B[0m";
        let green = "\x1B[42m/\x1B[0m";
        let fs = MockFs::new().symlink("dir/link", "../other").file("other");
        let res = resolve_with(fs, "dir/link");
        assert_eq!(
            render(&layout(&res, true), true),
            vec![
                format!("./dir{}link{}", green, red_sp),       // green dirname, red space
                format!("./dir{}../other{}", green, red_sp),   // debug: green + red space
                format!("   ./other{}", red_sp),
            ],
        );
    }

    #[test]
    fn render_verbose_color_debug_has_green_dirname_and_red_suffix() {
        // The verbatim symlink target (`../other`) is exactly the text between the
        // green dirname boundary and the red suffix boundary on the debug line.
        let red = "\x1B[41m/\x1B[0m";
        let green = "\x1B[42m/\x1B[0m";
        let fs = MockFs::new().symlink("dir/link", "../other").file("other/x");
        let res = resolve_with(fs, "dir/link/x");
        assert_eq!(
            render(&layout(&res, true), true),
            vec![
                format!("./dir{}link{}x ", green, red),       // normal: green + red
                format!("./dir{}../other{}x", green, red),    // debug: green..red = target
                format!("   ./other{}x ", red),               // terminal normal line
            ],
        );
    }

    #[test]
    fn render_verbose_color_absolute_debug_has_green_root_and_red_suffix() {
        // The absolute debug line carries a green root `/` (aligned under the
        // normal line's green) and a red suffix boundary; the verbatim target runs
        // from green to red, preserving the invariant that every debug line is
        // green-marked. Both greens sit in column 2 (under `/a`).
        let red = "\x1B[41m/\x1B[0m";
        let green = "\x1B[42m/\x1B[0m";
        let fs = MockFs::new().symlink("/a/link", "/x/y").file("/x/y/z");
        let res = resolve_with(fs, "/a/link/z");
        assert_eq!(
            render(&layout(&res, true), true),
            vec![
                format!("/a{}link{}z ", green, red),     // normal: green dirname, red suffix
                format!("  {}x/y{}z", green, red),       // debug: green root, red suffix
                format!("   /x/y{}z ", red),
            ],
        );
    }

    #[test]
    fn render_verbose_every_debug_line_has_green_and_red() {
        // Invariant: in verbose colored output, every debug line carries both a
        // green and a red mark — including no-suffix lines, where red lands on the
        // terminator space. This chain mixes a relative link with suffix, a
        // no-suffix relative link, and a no-suffix absolute link.
        let fs = MockFs::new()
            .symlink("dir/a", "b")        // relative, suffix `c`
            .symlink("dir/b/c", "/x/y")   // absolute, no suffix
            .file("/x/y");
        let res = resolve_with(fs, "dir/a/c");
        let lines = render(&layout(&res, true), true);
        // The debug lines are the even-indexed rows (each link's second line).
        let debug_lines: Vec<&String> = lines.iter()
            .enumerate()
            .filter(|(i, _)| i % 2 == 1)
            .map(|(_, l)| l)
            .collect();
        assert!(!debug_lines.is_empty());
        for line in debug_lines {
            assert!(line.contains(GREEN), "debug line missing green: {:?}", line);
            assert!(line.contains(RED), "debug line missing red: {:?}", line);
        }
    }

    // --- expand_path ----------------------------------------------------

    #[test]
    fn expand_path_is_identity_for_multi_component() {
        assert_eq!(expand_path("a/b/c"), PathBuf::from("a/b/c"));
    }
}
