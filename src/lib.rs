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

fn colorize(s: &str) -> String {
    if cfg!(target_os = "linux") && unsafe { libc::isatty(libc::STDOUT_FILENO as i32) } != 0 {
        format!("\x1B[31m{}\x1B[0m", s)
    } else {
        s.to_owned()
    }
}

fn format_symlink(f: &mut fmt::Formatter, source: &PathBuf, suffix: Option<&PathBuf>) -> fmt::Result {
    match suffix {
        Some(suffix) => write!(f, "{}{}{}", source.display(), colorize("/"), suffix.display()),
        None         => write!(f, "{}", source.display()),
    }
}

impl fmt::Display for SymlinkPath {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            NotLink (path) => format_symlink(f, path, None),
            Symlink { source, target: _, suffix } =>
                format_symlink(f, source, Some(suffix).filter(|_| suffix.iter().next().is_some())),
        }
    }
}

fn readlink<P: AsRef<Path>>(p: P) -> Result<Option<PathBuf>> {
    let metadata = fs::symlink_metadata(&p)?;
    if !metadata.file_type().is_symlink() {
        Ok(None)
    } else {
        let target = fs::read_link(&p)?;
        Ok(Some(target))
    }
}

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

impl Iterator for ReadlinksIterator {
    type Item = SymlinkPath;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done { return None }

        match find_symlink(&self.path) {
            Ok(symlink_path) => {
                if let NotLink(_) = symlink_path { self.done = true; }
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

pub fn resolve<P:AsRef<Path>>(p: P) -> ReadlinksIterator {
    ReadlinksIterator { path: p.as_ref().to_path_buf(), done: false }
}

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

    // TODO: do not rely on /nix/store symlinks.

    #[test]
    fn readlink_works() {
        let link = "/run/current-system";
        match readlink(link) {
            Ok(Some(path)) => assert!(path.starts_with("/nix/store")),
            _ => assert!(false, format!("{} should be a valid symlink!", link))
        }
    }

    #[test]
    fn find_symlink_works() {
        let link = Path::new("/run/current-system");
        match find_symlink(link) {
            Ok(Some(s)) => {
                assert_eq!(s.source, link);
                assert!(s.target.starts_with("/nix/store/"));
                assert_eq!(s.suffix, Path::new(""));
                assert_eq!(s.resolved, s.target);
            }
            _ => assert!(false)
        }
    }

    #[test]
    fn expand_path_works() {
        let expanded = expand_path(&"env");
        assert!(expanded.is_absolute());
    }
}
