//! [Oxen](https://github.com/Oxen-AI/Oxen) as the snapshot store: a folder of blobs and
//! manifests (the same keys a [`DirStore`] uses) that is also an Oxen repository, with a commit
//! for every snapshot. So the world's snapshots work as they do on a plain folder, and Oxen
//! adds a history of them (diffs, branches, copying a world) that outlives the week the
//! snapshots themselves are kept.
//!
//! Oxen can't rewrite history, so old history is [squashed](OxenStore::squash) by rebuilding
//! the repository: the newest commit before the cutoff becomes the base and the commits after
//! it are replayed on top, in a fresh repository swapped in for the old one.
//!
//! It runs the `oxen` command line (no RocksDB or DuckDB in our build), so the binary must be
//! installed: the container image has it. Native only.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::store::{BlobStore, DirStore};
use crate::{Error, err};

/// A day, s: history is squashed once there's this much of it past the cutoff.
const SQUASH_SLACK: u64 = 24 * 3600;

/// One commit: its id (Oxen's) and message (`<unix seconds, 12 digits> <note>`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Commit {
    pub id: String,
    pub message: String,
}

impl Commit {
    /// When it was made (from its message), if it says.
    pub fn at(&self) -> Option<u64> {
        self.message.split_once(' ').map_or(&*self.message, |(t, _)| t).parse().ok()
    }
}

pub struct OxenStore {
    dir: DirStore,
    root: PathBuf,
    oxen: PathBuf,
    /// where Oxen keeps its user settings (who commits): beside the store, not in a home
    /// folder that may not be there (or be writable)
    config: PathBuf,
}

impl OxenStore {
    /// The store at `root` (made if need be), with the `oxen` binary at `oxen`. A folder that
    /// already holds snapshots (a [`DirStore`]'s) becomes the repository's first commit.
    pub fn open(root: impl Into<PathBuf>, oxen: impl Into<PathBuf>) -> Result<OxenStore, Error> {
        let root: PathBuf = root.into();
        let mut config = root.clone().into_os_string();
        config.push(".oxen-config");
        let s = OxenStore { dir: DirStore::new(&root), root, oxen: oxen.into(), config: PathBuf::from(config) };
        std::fs::create_dir_all(&s.root)?;
        std::fs::create_dir_all(&s.config)?;
        s.run(&s.root, &["config", "--name", "vox", "--email", "vox@localhost"])?;
        if !s.root.join(".oxen").is_dir() {
            s.init(&s.root, "adopted what was here", 0)?;
        }
        Ok(s)
    }

    /// Run `oxen <args>` in `dir`; its output, or what it said went wrong.
    fn run(&self, dir: &Path, args: &[&str]) -> Result<String, Error> {
        let out = Command::new(&self.oxen)
            .args(args)
            .current_dir(dir)
            .env("OXEN_CONFIG_DIR", &self.config)
            .output()
            .map_err(|e| Error(format!("can't run {}: {e}", self.oxen.display())))?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).into_owned())
        } else {
            let said = format!("{}{}", String::from_utf8_lossy(&out.stderr), String::from_utf8_lossy(&out.stdout));
            err(format!("oxen {}: {}", args.join(" "), said.trim()))
        }
    }

    /// Make `dir` a repository and commit what's in it.
    fn init(&self, dir: &Path, note: &str, at: u64) -> Result<(), Error> {
        self.run(dir, &["init"])?;
        self.commit_in(dir, note, at)
    }

    fn commit_in(&self, dir: &Path, note: &str, at: u64) -> Result<(), Error> {
        self.run(dir, &["add", "."])?;
        self.run(dir, &["commit", "--allow-empty", "-m", &format!("{at:012} {note}")])?;
        Ok(())
    }

    /// The commits, oldest first.
    pub fn log(&self) -> Result<Vec<Commit>, Error> {
        let text = self.run(&self.root, &["log", "-n", "1000000"])?;
        let mut out = parse_log(&text);
        out.reverse();
        Ok(out)
    }

    /// Replace the history before `cutoff` (unix seconds) with its end state as one commit,
    /// keeping every commit from `cutoff` on. Only when there's at least a day of older
    /// history to drop (so a snapshot every five minutes doesn't rebuild every time). Returns
    /// how many commits were dropped.
    pub fn squash(&self, cutoff: u64) -> Result<usize, Error> {
        let log = self.log()?;
        // commits that don't say when (not ours) count as new: they're kept
        let older = log.iter().take_while(|c| c.at().is_some_and(|t| t < cutoff)).count();
        if older < 2 || log[0].at().is_none_or(|t| t + SQUASH_SLACK > cutoff) {
            return Ok(0);
        }
        // the newest old commit is the new base; everything after it is replayed
        let (base, replay) = (&log[older - 1], &log[older..]);
        let mut fresh = self.root.clone().into_os_string();
        fresh.push(".squashing");
        let fresh = PathBuf::from(fresh);
        let _ = std::fs::remove_dir_all(&fresh);
        std::fs::create_dir_all(&fresh)?;
        let result = (|| -> Result<(), Error> {
            for (i, c) in std::iter::once(base).chain(replay.iter()).enumerate() {
                // the old repository at that commit, copied into the new one
                self.run(&self.root, &["checkout", &c.id])?;
                mirror(&self.root, &fresh)?;
                if i == 0 {
                    self.run(&fresh, &["init"])?;
                }
                self.run(&fresh, &["add", "."])?;
                self.run(&fresh, &["commit", "--allow-empty", "-m", &c.message])?;
            }
            Ok(())
        })();
        if let Err(e) = result {
            // leave the old repository as it was (back on its newest commit)
            let _ = self.run(&self.root, &["checkout", "main"]);
            let _ = std::fs::remove_dir_all(&fresh);
            return Err(e);
        }
        // swap: the old one aside, the new one in, the old one gone
        let mut old = self.root.clone().into_os_string();
        old.push(".squashed");
        let old = PathBuf::from(old);
        let _ = std::fs::remove_dir_all(&old);
        std::fs::rename(&self.root, &old)?;
        if let Err(e) = std::fs::rename(&fresh, &self.root) {
            let _ = std::fs::rename(&old, &self.root);
            return Err(e.into());
        }
        let _ = std::fs::remove_dir_all(&old);
        Ok(older - 1)
    }
}

impl BlobStore for OxenStore {
    fn put(&self, key: &str, bytes: &[u8]) -> Result<(), Error> {
        self.dir.put(key, bytes)
    }
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>, Error> {
        self.dir.get(key)
    }
    fn delete(&self, key: &str) -> Result<(), Error> {
        self.dir.delete(key)
    }
    fn list(&self, prefix: &str) -> Result<Vec<String>, Error> {
        self.dir.list(prefix)
    }
    fn commit(&self, at: u64, note: &str) -> Result<(), Error> {
        self.commit_in(&self.root, note, at)
    }
    fn squash(&self, cutoff: u64) -> Result<usize, Error> {
        OxenStore::squash(self, cutoff)
    }
}

/// Make `to`'s files (everything but `.oxen`) the same as `from`'s.
fn mirror(from: &Path, to: &Path) -> Result<(), Error> {
    for e in std::fs::read_dir(to)?.flatten() {
        if e.file_name() != ".oxen" {
            let p = e.path();
            if p.is_dir() { std::fs::remove_dir_all(p)? } else { std::fs::remove_file(p)? }
        }
    }
    copy_tree(from, to)
}

fn copy_tree(from: &Path, to: &Path) -> Result<(), Error> {
    for e in std::fs::read_dir(from)?.flatten() {
        if e.file_name() == ".oxen" {
            continue;
        }
        let (src, dst) = (e.path(), to.join(e.file_name()));
        if src.is_dir() {
            std::fs::create_dir_all(&dst)?;
            copy_tree(&src, &dst)?;
        } else {
            std::fs::copy(&src, &dst)?;
        }
    }
    Ok(())
}

/// `oxen log`'s text: "commit <id>", then who and when, then the message indented, for each
/// commit, newest first.
pub fn parse_log(text: &str) -> Vec<Commit> {
    let mut out: Vec<Commit> = Vec::new();
    let mut id: Option<String> = None;
    for line in text.lines() {
        if let Some(i) = line.strip_prefix("commit ") {
            id = Some(i.trim().to_string());
        } else if let (Some(msg), true) = (line.strip_prefix("    "), id.is_some()) {
            out.push(Commit { id: id.take().unwrap_or_default(), message: msg.trim().to_string() });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The `oxen` to test with: `OXEN`, or one on the PATH; None (the tests skip) without.
    fn oxen() -> Option<PathBuf> {
        let candidates = std::env::var_os("OXEN").map(PathBuf::from).into_iter().chain(
            std::env::var_os("PATH").into_iter().flat_map(|p| std::env::split_paths(&p).map(|d| d.join("oxen")).collect::<Vec<_>>()),
        );
        let found = candidates.into_iter().find(|p| p.is_file());
        if found.is_none() {
            eprintln!("skipped: no oxen on the PATH (set OXEN)");
        }
        found
    }

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("vox-oxen-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let _ = std::fs::remove_dir_all(format!("{}.oxen-config", d.display()));
        d
    }

    #[test]
    fn the_log_is_read() {
        let text = "commit abc\n\nAuthor: vox\nDate:   Thursday\n\n    000000000005 snap two\n\ncommit def\n\nAuthor: vox\nDate:   Thursday\n\n    000000000001 snap one\n";
        let l = parse_log(text);
        assert_eq!(l.len(), 2);
        assert_eq!((l[0].id.as_str(), l[0].at()), ("abc", Some(5)));
        assert_eq!(l[1].message, "000000000001 snap one");
        assert_eq!(Commit { id: "x".into(), message: "no time here".into() }.at(), None);
    }

    #[test]
    fn a_commit_per_snapshot_and_old_history_squashed() {
        let Some(oxen) = oxen() else { return };
        let dir = temp("squash");
        let s = OxenStore::open(&dir, &oxen).unwrap();
        let day = 24 * 3600;
        // ten snapshots, a day and a bit apart, each adding a blob; the third goes later
        for i in 0..10u64 {
            s.put(&format!("blobs/b{i}"), format!("blob {i}").as_bytes()).unwrap();
            s.commit(1000 + i * (day + 100), &format!("snap {i}")).unwrap();
        }
        s.delete("blobs/b0").unwrap();
        s.commit(1000 + 10 * (day + 100), "drop b0").unwrap();
        assert_eq!(s.log().unwrap().len(), 12, "the adoption commit and eleven more");
        // squash everything before the seventh snapshot
        let cutoff = 1000 + 6 * (day + 100);
        let gone = s.squash(cutoff).unwrap();
        assert!(gone > 0);
        let log = s.log().unwrap();
        assert!(log[0].at().unwrap() < cutoff, "the base is the newest old commit: {:?}", log[0]);
        assert!(log.len() < 12 && log.len() >= 5, "{log:?}");
        assert!(log[1..].iter().all(|c| c.at().unwrap() >= cutoff), "everything from the cutoff kept: {log:?}");
        assert_eq!(log.last().unwrap().message, format!("{:012} drop b0", 1000 + 10 * (day + 100)));
        // the files are what the newest commit had: b0 gone, the rest there
        assert_eq!(s.get("blobs/b0").unwrap(), None);
        assert_eq!(s.get("blobs/b9").unwrap().unwrap(), b"blob 9");
        assert_eq!(s.list("blobs/").unwrap().len(), 9);
        // nothing left to squash: no rebuild
        assert_eq!(s.squash(cutoff).unwrap(), 0);
        // and a store reopened at the same place carries on
        let again = OxenStore::open(&dir, &oxen).unwrap();
        assert_eq!(again.log().unwrap().len(), log.len());
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(format!("{}.oxen-config", dir.display()));
    }

    #[test]
    fn a_folder_of_snapshots_is_adopted() {
        let Some(oxen) = oxen() else { return };
        let dir = temp("adopt");
        // what a DirStore left
        DirStore::new(&dir).put("snapshots/x.json", b"{}").unwrap();
        let s = OxenStore::open(&dir, &oxen).unwrap();
        assert_eq!(s.get("snapshots/x.json").unwrap().unwrap(), b"{}");
        assert_eq!(s.log().unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(format!("{}.oxen-config", dir.display()));
    }
}
