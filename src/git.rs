use crate::worker::Cancel;
use std::{
    collections::HashMap,
    io::{self, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[derive(Clone, Default)]
pub struct Snapshot {
    pub root: PathBuf,
    pub branch: String,
    pub changes: HashMap<PathBuf, String>,
    pub warning: Option<String>,
}
impl Snapshot {
    pub fn label(&self, path: &Path) -> &str {
        self.changes.get(path).map(String::as_str).unwrap_or("  ")
    }
}
#[derive(Default)]
pub struct Cache {
    entries: HashMap<PathBuf, (Instant, Snapshot)>,
}

// Read pipes incrementally: bounded memory, deadline and cancellation even if Git hangs.
fn output(path: &Path, args: &[&str], cancel: &Cancel, deadline: Instant) -> io::Result<Vec<u8>> {
    let mut command = Command::new("git");
    command
        .arg("--no-optional-locks")
        .arg("-c")
        .arg("core.fsmonitor=false")
        .arg("-c")
        .arg("core.untrackedCache=false")
        .arg("-C")
        .arg(path)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt;
        let parent = std::process::id();
        // Only async-signal-safe syscalls run between fork and exec. Linux kills
        // a running Git child even if the UI exits before the worker observes cancellation.
        unsafe {
            command.pre_exec(move || {
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                    return Err(io::Error::last_os_error());
                }
                if libc::getppid() != parent as libc::pid_t {
                    libc::_exit(1);
                }
                Ok(())
            });
        }
    }
    let mut child = command.spawn()?;
    let mut pipe = child.stdout.take().unwrap();
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        // The descriptor is owned and remains open for the duration of these calls.
        unsafe {
            let flags = libc::fcntl(pipe.as_raw_fd(), libc::F_GETFL);
            if flags < 0
                || libc::fcntl(pipe.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) < 0
            {
                let error = io::Error::last_os_error();
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        }
    }
    let result = (|| {
        let mut bytes = Vec::new();
        let mut chunk = [0; 8192];
        loop {
            if cancel.cancelled() || Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Git exceeded its 600 ms budget or was cancelled",
                ));
            }
            match pipe.read(&mut chunk) {
                Ok(0) => {
                    if let Some(status) = child.try_wait()? {
                        return if status.success() {
                            Ok(bytes)
                        } else {
                            Err(io::Error::other("Git status unavailable"))
                        };
                    }
                }
                Ok(n) => {
                    bytes.extend_from_slice(&chunk[..n]);
                    if bytes.len() > 2 * 1024 * 1024 {
                        return Err(io::Error::other("Git status exceeds 2 MiB"));
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => (),
                Err(e) => return Err(e),
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    })();
    if result.is_err() {
        let _ = child.kill();
    }
    let _ = child.wait();
    result
}

fn bytes_path(bytes: &[u8]) -> PathBuf {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        std::ffi::OsString::from_vec(bytes.to_vec()).into()
    }
    #[cfg(not(unix))]
    PathBuf::from(String::from_utf8_lossy(bytes).as_ref())
}

fn parse(root: PathBuf, bytes: &[u8]) -> Snapshot {
    let mut result = Snapshot {
        root: root.clone(),
        ..Snapshot::default()
    };
    let mut records = bytes.split(|b| *b == 0);
    while let Some(record) = records.next() {
        if let Some(branch) = record.strip_prefix(b"## ") {
            result.branch = String::from_utf8_lossy(branch).into();
            continue;
        }
        if record.len() < 4 {
            continue;
        }
        let status = String::from_utf8_lossy(&record[..2]).into_owned();
        let path = root.join(bytes_path(&record[3..]));
        result.changes.insert(path.clone(), status);
        let mut parent = path.parent();
        while let Some(p) = parent.filter(|p| *p != root && p.starts_with(&root)) {
            result
                .changes
                .entry(p.into())
                .or_insert_with(|| "M*".into());
            parent = p.parent();
        }
        if record[..2].contains(&b'R') || record[..2].contains(&b'C') {
            records.next();
        }
    }
    result
}

impl Cache {
    pub fn scan(&mut self, path: &Path, force: bool, cancel: &Cancel) -> Snapshot {
        let deadline = Instant::now() + Duration::from_millis(600);
        let result = (|| {
            let bytes = output(path, &["rev-parse", "--show-toplevel"], cancel, deadline)?;
            let root = bytes_path(bytes.strip_suffix(b"\n").unwrap_or(&bytes));
            if !force
                && let Some((time, snapshot)) = self.entries.get(&root)
                && time.elapsed() < Duration::from_secs(2)
            {
                return Ok((snapshot.clone(), false));
            }

            let bytes = output(
                &root,
                &[
                    "status",
                    "--porcelain=v1",
                    "--branch",
                    "-z",
                    "--no-renames",
                    "--untracked-files=normal",
                    "--ignore-submodules=all",
                ],
                cancel,
                deadline,
            )?;
            Ok::<_, io::Error>((parse(root, &bytes), true))
        })();
        match result {
            Ok((snapshot, fresh)) => {
                if fresh {
                    if self.entries.len() >= 8 {
                        self.entries.clear();
                    }
                    self.entries
                        .insert(snapshot.root.clone(), (Instant::now(), snapshot.clone()));
                }
                snapshot
            }
            Err(e) => Snapshot {
                warning: Some(e.to_string()),
                ..Snapshot::default()
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_nul_paths_and_aggregates_directory_changes() {
        let data = b"## main\0 M src/a\nfile.rs\0?? new file\0R  moved\0old\0";
        let s = parse("/repo".into(), data);
        assert_eq!(s.branch, "main");
        assert_eq!(s.label(Path::new("/repo/src")), "M*");
        assert_eq!(s.label(Path::new("/repo/src/a\nfile.rs")), " M");
        assert_eq!(s.label(Path::new("/repo/new file")), "??");
        assert!(!s.changes.contains_key(Path::new("/repo/old")));
    }
}
