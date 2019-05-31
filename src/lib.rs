extern crate libc;

use std::fs;
use std::env;
use std::fmt;
use std::path::Path;
use std::path::PathBuf;
use std::io::Result;

pub enum SymlinkPath {
    Symlink {
        source: PathBuf,
        target: PathBuf,
        suffix: PathBuf,
    },
    NotLink (PathBuf),
}
use SymlinkPath::*;

impl SymlinkPath {
    fn resolve(&self) -> PathBuf {
        match self {
            NotLink(p) => p.clone(),
            Symlink{ source, target, suffix } => {
                let mut resolved = source.as_path().parent().unwrap().to_path_buf();
                resolved.push(target.as_path());
                if suffix.iter().next().is_some() {
                    resolved.push(suffix.as_path());
                }
                resolved
            },
        }
    }
}



// # Formatting

/// Add ansi escape codes to make the given string red when stdout is a terminal.
fn colorize(s: &str) -> String {
    if cfg!(target_os = "linux") && unsafe { libc::isatty(libc::STDOUT_FILENO as i32) } != 0 {
        format!("\x1B[31m{}\x1B[0m", s)
    } else {
        s.to_owned()
    }
}

fn format_symlink(f: &mut fmt::Formatter, source: &PathBuf, suffix: Option<&PathBuf>, exists: bool) -> fmt::Result {
    let exists = if exists {""} else {" (not found)"};
    // Test if we have a non-empty suffix
    match suffix.filter(|s| s.iter().next().is_some()) {
        Some(suffix) => write!(f, "{}{}{}{}", source.display(), colorize("/"), suffix.display(), exists),
        None         => write!(f, "{}{}", source.display(), exists),
    }
}

impl fmt::Display for SymlinkPath {
    /// Rich formatter for symlinks
    ///
    /// * Tag a `NotLink` path when it does not exist
    /// * On ansi tty, separate the first symlink from the remainder of the path with a red slash.
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            NotLink (path) =>
                format_symlink(f, path, None, path.metadata().is_ok()),
            Symlink { source, target: _, suffix } =>
                format_symlink(f, source, Some(suffix), true),
        }
    }
}


/// Query the symbolic target of a `path`.
///
/// This is a generic wrapper around `read_link()` to also check whether it's a symlink
/// before reading its target. The path need not be a symlink, in which case `None` is returned.
fn readlink<P: AsRef<Path>>(path: P) -> Result<Option<PathBuf>> {
    let metadata = fs::symlink_metadata(&path)?;
    if !metadata.file_type().is_symlink() {
        Ok(None)
    } else {
        let target = fs::read_link(&path)?;
        Ok(Some(target))
    }
}

/// Find the first symlink in a `path`.
///
/// Works by iteratively
fn find_symlink<P: AsRef<Path>>(path: P) -> Result<SymlinkPath>{
    let mut prefix = PathBuf::new();
    let mut parts = path.as_ref().components();
    while let Some(part) = parts.next() {
        prefix.push(&part);
        if let Some(target) = readlink(&prefix)? {
            return Ok(Symlink {
                source: prefix,
                target,
                suffix: parts.as_path().into()
            })
        }
    }
    Ok(NotLink(prefix))
}

pub struct ReadlinksIterator {
    path: PathBuf,
    done: bool,
}

/// Readlinks path lookup logic
///
/// This works by iteratively calling find_symlink and expanding the result.
impl Iterator for ReadlinksIterator {
    type Item = SymlinkPath;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None
        }
        match find_symlink(&self.path) {
            Ok(symlink_path) => {
                // When resolution is finished, we still need one iteration to print the final result
                if let NotLink(_) = symlink_path {
                    self.done = true;
                }
                self.path = symlink_path.resolve();
                Some(symlink_path)
            },
            Err(ref e) if e.kind() == std::io::ErrorKind::NotFound => {
                self.done = true;
                Some(NotLink(self.path.clone()))
            }
            // TODO: Move error handling to main.rs
            Err(e) => {
                eprintln!("Accessing {} failed: {}", self.path.display(), e);
                None
            },
        }
    }
}

/// List all the intermediate symlinks in the resolution of `path`.
pub fn resolve<P:AsRef<Path>>(path: P) -> ReadlinksIterator {
    ReadlinksIterator { path: path.as_ref().to_path_buf(), done: false }
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

