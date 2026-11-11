use clap::ValueEnum;
use serde::Serialize;
use std::{
    fs, io,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum Sort {
    Name,
    Size,
    Modified,
}

#[derive(Clone, Debug, Serialize)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub kind: &'static str,
    pub size: u64,
    pub modified: Option<u64>,
    pub link_target: Option<PathBuf>,
    pub permissions: u32,
    pub directory: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_status: Option<String>,
}

fn entry(path: PathBuf) -> io::Result<Entry> {
    let meta = fs::symlink_metadata(&path)?;
    let link = meta.file_type().is_symlink();
    #[cfg(unix)]
    let permissions = {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o7777
    };
    #[cfg(not(unix))]
    let permissions = 0;
    Ok(Entry {
        name: path
            .file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy()
            .into(),
        directory: meta.is_dir() || (link && path.is_dir()),
        kind: if link {
            "link"
        } else if meta.is_dir() {
            "dir"
        } else if meta.is_file() {
            "file"
        } else {
            "special"
        },
        size: meta.len(),
        modified: meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs()),
        link_target: if link {
            fs::read_link(&path).ok()
        } else {
            None
        },
        path,
        permissions,
        git_status: None,
    })
}

pub fn read(path: &Path, all: bool, sort: Sort) -> io::Result<Vec<Entry>> {
    read_cancel(path, all, sort, &crate::worker::Cancel::fresh())
}

pub fn read_cancel(
    path: &Path,
    all: bool,
    sort: Sort,
    cancel: &crate::worker::Cancel,
) -> io::Result<Vec<Entry>> {
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_dir() {
        return Ok(vec![entry(path.into())?]);
    }
    let mut entries = Vec::new();
    for child in fs::read_dir(path)? {
        if cancel.cancelled() {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "directory load cancelled",
            ));
        }
        let child = child?;
        if !all && child.file_name().as_encoded_bytes().first() == Some(&b'.') {
            continue;
        }
        // Surface metadata errors instead of returning an apparently complete list.
        entries.push(entry(child.path())?);
    }
    entries.sort_by_cached_key(|entry| {
        (
            !entry.directory,
            match sort {
                Sort::Name => 0,
                Sort::Size => u64::MAX - entry.size,
                Sort::Modified => u64::MAX - entry.modified.unwrap_or(0),
            },
            entry.name.to_lowercase(),
            entry.path.clone(),
        )
    });
    Ok(entries)
}

pub fn size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn folders_first_and_hidden_opt_in() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), "abc").unwrap();
        fs::write(dir.path().join(".hidden"), "").unwrap();
        fs::create_dir(dir.path().join("z-folder")).unwrap();
        let entries = read(dir.path(), false, Sort::Name).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "z-folder");
        assert_eq!(read(dir.path(), true, Sort::Name).unwrap().len(), 3);
    }
    #[test]
    fn file_argument_and_size_sort() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a");
        fs::write(&file, "abc").unwrap();
        fs::write(dir.path().join("b"), "longer").unwrap();
        assert_eq!(read(&file, false, Sort::Name).unwrap()[0].size, 3);
        assert_eq!(read(dir.path(), false, Sort::Size).unwrap()[0].name, "b");
    }
    #[cfg(unix)]
    #[test]
    fn broken_symlink_is_preserved() {
        let dir = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink("missing", dir.path().join("link")).unwrap();
        let entries = read(dir.path(), false, Sort::Name).unwrap();
        assert_eq!(entries[0].kind, "link");
        assert_eq!(
            entries[0].link_target.as_deref(),
            Some(Path::new("missing"))
        );
    }
    #[test]
    fn human_size() {
        assert_eq!(size(1024), "1.0 KiB");
        assert_eq!(size(0), "0 B");
    }
}
