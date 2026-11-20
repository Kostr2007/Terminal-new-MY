use crate::{
    files::{self, Entry},
    render,
    worker::Cancel,
};
use std::{
    fs::OpenOptions,
    io::{self, Read},
};

const TEXT_LIMIT: usize = 64 * 1024;
const PNG_LIMIT: usize = 2 * 1024 * 1024;

pub struct Png {
    pub size: (usize, usize),
    pub data: Vec<u8>,
}

pub struct Content {
    pub text: String,
    pub png: Option<Png>,
}

enum Data {
    Bytes(Vec<u8>),
    Png(Png),
}

fn png_size(bytes: &[u8]) -> Option<(usize, usize)> {
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR") {
        return None;
    }
    let width = u32::from_be_bytes(bytes.get(16..20)?.try_into().ok()?);
    let height = u32::from_be_bytes(bytes.get(20..24)?.try_into().ok()?);
    ((1..=2048).contains(&width) && (1..=2048).contains(&height))
        .then_some((width as usize, height as usize))
}

fn read_to(
    file: &mut impl Read,
    bytes: &mut Vec<u8>,
    limit: usize,
    cancel: &Cancel,
) -> io::Result<()> {
    let mut chunk = [0; 4096];
    while bytes.len() < limit && !cancel.cancelled() {
        let size = chunk.len().min(limit - bytes.len());
        let n = file.read(&mut chunk[..size])?;
        if n == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..n]);
    }
    Ok(())
}

pub fn load(entry: Entry, cancel: &Cancel, graphics: bool) -> Content {
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
        return Content { text, png: None };
    }
    let result = (|| -> io::Result<Data> {
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
        }
        let mut file = options.open(&entry.path)?;
        let meta = file.metadata()?;
        if !meta.is_file() {
            return Ok(Data::Bytes(Vec::new()));
        }
        let mut bytes = Vec::new();
        read_to(&mut file, &mut bytes, TEXT_LIMIT, cancel)?;
        if graphics
            && meta.len() <= PNG_LIMIT as u64
            && let Some(size) = png_size(&bytes)
        {
            read_to(&mut file, &mut bytes, PNG_LIMIT + 1, cancel)?;
            if bytes.len() <= PNG_LIMIT && bytes.ends_with(b"\0\0\0\0IEND\xaeB\x60\x82") {
                return Ok(Data::Png(Png { size, data: bytes }));
            }
            bytes.truncate(TEXT_LIMIT);
        }
        Ok(Data::Bytes(bytes))
    })();
    match result {
        Ok(Data::Png(png)) => {
            let (width, height) = png.size;
            text.push_str(&format!("\nPNG · {width} × {height}\n"));
            return Content {
                text,
                png: Some(png),
            };
        }
        Ok(Data::Bytes(bytes)) if bytes.contains(&0) => {
            text.push_str("\nBinary file · text preview unavailable")
        }
        Ok(Data::Bytes(bytes)) => {
            text.push('\n');
            for line in String::from_utf8_lossy(&bytes).lines().take(2000) {
                if cancel.cancelled() {
                    break;
                }
                text.push_str(&render::safe(line));
                text.push('\n');
            }
            if entry.size > TEXT_LIMIT as u64
                || bytes.iter().filter(|b| **b == b'\n').count() > 2000
            {
                text.push_str("\n[preview limited to 64 KiB / 2000 lines]");
            }
        }
        Err(e) => text.push_str(&format!("\nPreview: {}", render::safe(&e.to_string()))),
    }
    Content { text, png: None }
}
