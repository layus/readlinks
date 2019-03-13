use std::fs;
use std::env;
use std::path::Path;
use std::path::PathBuf;
use std::io::Result;

fn readlink<P: AsRef<Path>>(p: P) -> Result<Option<PathBuf>> {
    let metadata = fs::symlink_metadata(&p)?;
    if !metadata.file_type().is_symlink() {
        Ok(None)
    } else {
        let target = fs::read_link(&p)?;
        Ok(Some(target))
    }
}

fn find_symlink<P: AsRef<Path>>(path: P) -> Result<Option<Symlink>>{
    let mut prefix = PathBuf::new();
    let mut parts = path.as_ref().iter();
    while let Some(part) = parts.next() {
        prefix.push(&part);
        if let Some(target) = readlink(&prefix)? {
            return Ok(Some(Symlink::new(prefix, target, parts.collect())))
        }
    }
    Ok(None)
}

pub struct Symlink {
    pub source: PathBuf,
    pub target: PathBuf,
    pub suffix: PathBuf,
    pub resolved: PathBuf,
}

impl Symlink {
    pub fn new(source: PathBuf, target: PathBuf, suffix: PathBuf) -> Symlink {
        let mut resolved = source.as_path().parent().unwrap().to_path_buf();
        // XXX: Unwrap looks safe here as the root cannot be a symlink
        // and is the only path with no parent.
        resolved.push(target.as_path());
        if suffix.iter().next().is_some() {
            resolved.push(suffix.as_path());
        }

        Symlink { source, target, suffix, resolved, }
    }
}

pub struct ReadlinksIterator {
    path: PathBuf,
}

impl Iterator for ReadlinksIterator {
    type Item = Symlink;

    fn next(&mut self) -> Option<Self::Item> {
        match find_symlink(&self.path) {
            Ok(Some(s)) => {
                self.path = s.resolved.clone();
                Some(s)
            },
            Ok(None) => None,

            // TODO: Move error handling to main.rs
            Err(ref e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => {
                eprintln!("Accessing {} failed: {}", self.path.display(), e);
                None
            },
        }
    }
}

pub fn resolve<P:AsRef<Path>>(p: P) -> ReadlinksIterator {
    ReadlinksIterator { path: p.as_ref().to_path_buf() }
}

pub fn expand_path<P: AsRef<Path>>(bin: P) -> PathBuf {
    let bin = bin.as_ref();
    env::var_os("PATH").and_then(|ref paths|
        env::split_paths(paths)
        .map(|p| p.join(bin))
        .find(|p| p.is_file())
    ).unwrap_or_else(|| bin.to_path_buf())
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
