use crate::{
    files::{self, Entry},
    render,
    worker::Cancel,
};
use std::{
    fs::OpenOptions,
    io::{self, Read},
};

pub fn load(entry: Entry, cancel: &Cancel) -> String {
    let mut text = format!(
        "{}\n\n{} · {}\nPermissions: {:04o}\n",
        render::safe(&entry.name),
        entry.kind,
        files::size(entry.size),
        entry.permissions
    );
    if let Some(target) = &entry.link_target {
        text.push_str(&format!(
            "\nLink → {}\n",
            render::safe(&target.to_string_lossy())
        ));
    }
    if entry.kind != "file" || cancel.cancelled() {
        return text;
    }
    let result = (|| -> io::Result<Vec<u8>> {
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
        }
        let mut file = options.open(&entry.path)?;
        if !file.metadata()?.is_file() {
            return Ok(Vec::new());
        }
        let mut bytes = Vec::new();
        let mut chunk = [0; 4096];
        while bytes.len() < 64 * 1024 && !cancel.cancelled() {
            let n = file.read(&mut chunk)?;
            if n == 0 {
                break;
            }
            bytes.extend_from_slice(&chunk[..n]);
        }
        Ok(bytes)
    })();
    match result {
        Ok(bytes) if bytes.contains(&0) => {
            text.push_str("\nBinary file · text preview unavailable")
        }
        Ok(bytes) => {
            text.push('\n');
            for line in String::from_utf8_lossy(&bytes).lines().take(2000) {
                if cancel.cancelled() {
                    break;
                }
                text.push_str(&render::safe(line));
                text.push('\n');
            }
            if entry.size > 64 * 1024 || bytes.iter().filter(|b| **b == b'\n').count() > 2000 {
                text.push_str("\n[preview limited to 64 KiB / 2000 lines]");
            }
        }
        Err(e) => text.push_str(&format!("\nPreview: {}", render::safe(&e.to_string()))),
    }
    text
}
