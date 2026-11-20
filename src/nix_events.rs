use serde_json::Value;
use std::{
    collections::HashMap,
    fs::File,
    io::{self, BufRead, BufReader},
    path::Path,
};

#[derive(Default)]
struct Progress {
    done: u64,
    failed: u64,
}

#[derive(Default)]
pub struct Summary {
    builds: Progress,
    copies: Progress,
    pub records: usize,
}

impl Summary {
    pub fn read(path: &Path) -> io::Result<Self> {
        let mut totals = Self::default();
        let mut activities = HashMap::new();
        let mut reader = BufReader::new(File::open(path)?);
        // Give oversized records a boundary without allocating an unbounded line.
        let mut line = Vec::new();
        loop {
            let length = reader
                .by_ref()
                .take(256 * 1024)
                .read_until(b'\n', &mut line)?;
            if length == 0 {
                break;
            }
            if line.last() != Some(&b'\n') && length == 256 * 1024 {
                reader.skip_until(b'\n')?;
            } else if let Ok(event) =
                serde_json::from_slice::<Value>(line.strip_prefix(b"@nix ").unwrap_or(&line))
            {
                totals.records += 1;
                let id = event["id"].as_u64();
                match (event["action"].as_str(), event["type"].as_u64(), id) {
                    (Some("start"), Some(kind @ (103 | 104)), Some(id)) => {
                        activities.insert(id, kind);
                    }
                    (Some("stop"), _, Some(id)) => {
                        activities.remove(&id);
                    }
                    (Some("result"), Some(105), Some(id)) => {
                        let fields = &event["fields"];
                        if let (Some(done), Some(_expected), Some(_running), Some(failed)) = (
                            fields[0].as_u64(),
                            fields[1].as_u64(),
                            fields[2].as_u64(),
                            fields[3].as_u64(),
                        ) {
                            let progress = Progress { done, failed };
                            match activities.get(&id) {
                                Some(103) if done != 0 || failed != 0 => totals.copies = progress,
                                Some(104) if done != 0 || failed != 0 => totals.builds = progress,
                                _ => (),
                            }
                        }
                    }
                    _ => (),
                }
            }
            line.clear();
        }
        Ok(totals)
    }

    pub fn report(&self) {
        for (label, progress) in [("Builds", &self.builds), ("Store copies", &self.copies)] {
            if progress.done != 0 || progress.failed != 0 {
                eprintln!(
                    "  {label}: {} complete · {} failed",
                    progress.done, progress.failed
                );
            }
        }
        eprintln!("  Nix events: {}", self.records);
    }
}

use std::io::Read;

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn aggregates_only_known_progress_and_retains_failure_counts() {
        let mut log = tempfile::NamedTempFile::new().unwrap();
        log.write_all(
            br#"@nix {"action":"start","id":1,"type":104}
{"action":"result","id":1,"type":105,"fields":[3,5,1,1]}
{"action":"result","id":2,"type":105,"fields":[99,99,0,0]}
{"action":"stop","id":1}
{"action":"msg","level":0,"msg":"builder failed"}
not JSON"#,
        )
        .unwrap();
        let summary = Summary::read(log.path()).unwrap();
        assert_eq!(summary.records, 5);
        assert_eq!(summary.builds.done, 3);
        assert_eq!(summary.builds.failed, 1);
        assert_eq!(summary.copies.done, 0);
    }
}
