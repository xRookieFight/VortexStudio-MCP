//! Loading and saving projects without ever leaving a half written file.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::vrtx::{self, Project};

const MAX_FILE: u64 = 256 << 20;
const KEEP_BACKUPS: usize = 20;
pub const BACKUP_DIR: &str = ".vrtx-backups";

type Result<T> = std::result::Result<T, String>;

/// Turns what a user or model typed into a real path. Handles `~` and Wine
/// style `Z:\home\me\game.vrtx`, since Studio under Wine shows paths that way.
pub fn normalize(input: &str) -> Result<PathBuf> {
    let s = input.trim();
    if s.is_empty() {
        return Err("file path is empty".into());
    }
    let unix_from_wine = s
        .strip_prefix("Z:\\")
        .or_else(|| s.strip_prefix("z:\\"))
        .or_else(|| s.strip_prefix("Z:/"))
        .filter(|_| cfg!(unix));
    let path = if let Some(rest) = unix_from_wine {
        PathBuf::from(format!("/{}", rest.replace('\\', "/")))
    } else if let Some(rest) = s.strip_prefix("~/") {
        let home = std::env::var_os("HOME").ok_or("HOME isn't set, use an absolute path")?;
        PathBuf::from(home).join(rest)
    } else {
        PathBuf::from(s)
    };
    Ok(path)
}

pub fn load(path: &Path) -> Result<Project> {
    let meta = fs::metadata(path).map_err(|e| format!("can't open {}: {e}", path.display()))?;
    if !meta.is_file() {
        return Err(format!("{} is not a file", path.display()));
    }
    if meta.len() > MAX_FILE {
        return Err(format!(
            "{} is larger than 256 MB, refusing to load it",
            path.display()
        ));
    }
    let bytes = fs::read(path).map_err(|e| format!("can't read {}: {e}", path.display()))?;
    vrtx::decode(&bytes).map_err(|e| format!("{}: {e}", path.display()))
}

/// Writes the project in the current format. The old file is copied into
/// `.vrtx-backups/` next to it first, the new bytes are decoded again to make
/// sure they read back identically, and the swap is a rename so a crash
/// can't leave a truncated project behind. Returns the backup path.
pub fn save(path: &Path, project: &Project) -> Result<Option<PathBuf>> {
    let bytes = vrtx::encode(project).map_err(|e| e.to_string())?;
    let check = vrtx::decode(&bytes)
        .map_err(|e| format!("refusing to save, the encoded project doesn't read back: {e}"))?;
    if check.instances != project.instances || check.lighting != project.lighting {
        return Err("refusing to save, the encoded project doesn't read back identically".into());
    }

    let backup = if path.exists() {
        Some(backup(path)?)
    } else {
        None
    };

    let dir = path
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let tmp = dir.join(format!(
        ".{}.tmp-{}",
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("project.vrtx"),
        std::process::id()
    ));
    fs::write(&tmp, &bytes).map_err(|e| format!("can't write {}: {e}", tmp.display()))?;
    if let Err(e) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(format!("can't replace {}: {e}", path.display()));
    }
    Ok(backup)
}

fn backup(path: &Path) -> Result<PathBuf> {
    let dir = backup_dir(path);
    fs::create_dir_all(&dir).map_err(|e| format!("can't create {}: {e}", dir.display()))?;
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("project");
    let mut target = dir.join(format!("{stem}-{}.vrtx", timestamp()));
    let mut n = 1;
    while target.exists() {
        target = dir.join(format!("{stem}-{}-{n}.vrtx", timestamp()));
        n += 1;
    }
    fs::copy(path, &target).map_err(|e| format!("can't back up to {}: {e}", target.display()))?;
    prune(&dir, stem);
    Ok(target)
}

pub fn backup_dir(path: &Path) -> PathBuf {
    path.parent().unwrap_or(Path::new(".")).join(BACKUP_DIR)
}

/// Backups of this project, newest first.
pub fn list_backups(path: &Path) -> Vec<PathBuf> {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("project");
    let prefix = format!("{stem}-");
    let mut found: Vec<PathBuf> = fs::read_dir(backup_dir(path))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
                n.starts_with(&prefix) && n.ends_with(".vrtx") && is_backup_name(&n[prefix.len()..])
            })
        })
        .collect();
    // names alone sort "-10" before "-2", modification time doesn't
    found.sort_by_key(|p| (fs::metadata(p).and_then(|m| m.modified()).ok(), p.clone()));
    found.reverse();
    found
}

// "20261007-174501.vrtx" or "20261007-174501-2.vrtx", so `level-` doesn't match `level-2-...`
fn is_backup_name(rest: &str) -> bool {
    let b = rest.as_bytes();
    b.len() >= 20
        && b[..8].iter().all(u8::is_ascii_digit)
        && b[8] == b'-'
        && b[9..15].iter().all(u8::is_ascii_digit)
}

fn prune(dir: &Path, stem: &str) {
    let fake = dir.parent().unwrap_or(dir).join(format!("{stem}.vrtx"));
    for old in list_backups(&fake).into_iter().skip(KEEP_BACKUPS) {
        let _ = fs::remove_file(old);
    }
}

fn timestamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs()) as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}{m:02}{d:02}-{:02}{:02}{:02}",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

// Howard Hinnant's days to civil date, UTC
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// Best effort check for a running Studio. If it has the project open, it
/// won't see our edits until reopened and may overwrite them on its next save.
pub fn studio_running() -> bool {
    #[cfg(target_os = "linux")]
    {
        fs::read_dir("/proc")
            .into_iter()
            .flatten()
            .flatten()
            .any(|e| {
                fs::read(e.path().join("cmdline"))
                    .map(|c| String::from_utf8_lossy(&c).contains("VortexStudio.exe"))
                    .unwrap_or(false)
            })
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("tasklist")
            .args(["/FI", "IMAGENAME eq VortexStudio.exe", "/NH"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains("VortexStudio.exe"))
            .unwrap_or(false)
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("pgrep")
            .args(["-f", "Vortex Studio"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene;

    #[test]
    fn wine_and_home_paths() {
        if cfg!(unix) {
            assert_eq!(
                normalize("Z:\\home\\me\\game.vrtx").unwrap(),
                PathBuf::from("/home/me/game.vrtx")
            );
        }
        assert_eq!(
            normalize(" /tmp/a.vrtx ").unwrap(),
            PathBuf::from("/tmp/a.vrtx")
        );
        assert!(normalize("  ").is_err());
    }

    #[test]
    fn save_backs_up_and_reads_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("game.vrtx");
        let mut p = scene::new_project("0123456789abcdef0123456789abcdef".into());
        assert_eq!(save(&path, &p).unwrap(), None);

        scene::rename(&mut p, 5, "Floor").unwrap();
        let backup = save(&path, &p).unwrap().unwrap();
        assert!(backup.starts_with(dir.path().join(BACKUP_DIR)));
        assert_eq!(load(&backup).unwrap().instances[5].name, "Baseplate");
        assert_eq!(load(&path).unwrap().instances[5].name, "Floor");
        assert_eq!(list_backups(&path), vec![backup]);
        // no temp files left lying around
        let names: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert_eq!(names.len(), 2, "{names:?}");
    }

    #[test]
    fn backups_are_pruned() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("game.vrtx");
        let p = scene::new_project("x".into());
        for _ in 0..KEEP_BACKUPS + 5 {
            save(&path, &p).unwrap();
        }
        assert_eq!(list_backups(&path).len(), KEEP_BACKUPS);
    }

    #[test]
    fn load_errors_are_readable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.vrtx");
        fs::write(&path, b"hello").unwrap();
        assert!(load(&path).unwrap_err().contains("not a .vrtx file"));
        assert!(
            load(&dir.path().join("missing.vrtx"))
                .unwrap_err()
                .contains("can't open")
        );
    }

    #[test]
    fn civil_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_733), (2026, 10, 7));
    }
}
