use crate::{kitty, nix_events, palette::Theme, render};
use clap::{Args as ClapArgs, ValueEnum};
use std::{
    ffi::OsString,
    io::{self, Write},
    process::{Command, Stdio},
    sync::{Arc, atomic::AtomicBool},
    time::Instant,
};

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum Action {
    Build,
    Test,
    Switch,
    Boot,
}
impl Action {
    fn name(self) -> &'static str {
        match self {
            Self::Build => "build",
            Self::Test => "test",
            Self::Switch => "switch",
            Self::Boot => "boot",
        }
    }
}

#[derive(ClapArgs)]
pub struct Args {
    /// Flake reference used by nh (otherwise nh reads its own environment)
    #[arg(long)]
    pub flake: Option<OsString>,
    /// Rebuild by default; explicitly request input updates with --refresh
    #[arg(long)]
    pub refresh: bool,
    #[arg(long, value_enum, default_value = "switch")]
    pub action: Action,
    /// Display the exact argument vector without executing nh
    #[arg(long)]
    pub dry_run: bool,
    /// Record private Nix events even outside Kitty (requires json-log-path support).
    #[arg(long)]
    pub observe: bool,
    /// Summarize a saved Nix event log without running nh.
    #[arg(long, conflicts_with_all = ["observe", "dry_run", "refresh", "flake", "extra"])]
    pub replay: Option<std::path::PathBuf>,
    /// Extra nh options, after --
    #[arg(last = true)]
    pub extra: Vec<OsString>,
}

pub fn available() -> bool {
    std::env::var_os("PATH").is_some_and(|path| {
        std::env::split_paths(&path).any(|p| {
            let path = p.join("nh");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                path.metadata()
                    .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            }
            #[cfg(not(unix))]
            path.is_file()
        })
    })
}

fn arguments(args: &Args, event_path: Option<&std::path::Path>) -> Vec<OsString> {
    let mut command = vec!["os".into(), args.action.name().into()];
    if args.refresh {
        command.push("--update".into());
    }
    if let Some(path) = event_path {
        command.extend(["--option".into(), "json-log-path".into(), path.into()]);
    }
    if let Some(flake) = &args.flake {
        command.push(flake.clone());
    }
    command.extend(args.extra.iter().cloned());
    command
}

fn recording(args: &Args) -> io::Result<tempfile::NamedTempFile> {
    let incompatible = args.extra.iter().any(|arg| {
        let arg = arg.to_string_lossy();
        ["--build-host", "--target-host", "json-log-path", "--"]
            .iter()
            .any(|name| arg == *name || arg.starts_with(&format!("{name}=")))
    });
    if incompatible {
        return Err(io::Error::other(
            "recording requires a local build and its own json-log-path",
        ));
    }
    let config = Command::new("nix")
        .args(["config", "show", "--json"])
        .output()?;
    let supported = config.status.success()
        && serde_json::from_slice::<serde_json::Value>(&config.stdout)
            .is_ok_and(|settings| settings.get("json-log-path").is_some());
    if !supported {
        return Err(io::Error::other("this Nix does not expose json-log-path"));
    }
    let state = std::env::var_os("XDG_STATE_HOME")
        .filter(|path| std::path::Path::new(path).is_absolute())
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(".local/state"))
        })
        .ok_or_else(|| io::Error::other("set HOME or XDG_STATE_HOME to record Nix events"))?;
    let dir = state.join("terminal-new-my/builds");
    std::fs::create_dir_all(&dir)?;
    tempfile::Builder::new()
        .prefix("nix-")
        .suffix(".jsonl")
        .tempfile_in(dir)
}

pub fn run(args: Args, styled: bool, theme: &Theme, graphics: bool) -> io::Result<u8> {
    if let Some(path) = &args.replay {
        nix_events::Summary::read(path)?.report();
        return Ok(0);
    }
    if args.dry_run {
        // A JSON argument vector is unambiguous even when paths contain shell syntax.
        let command: Vec<_> = std::iter::once(OsString::from("nh"))
            .chain(arguments(&args, None))
            .map(|s| s.to_string_lossy().into_owned())
            .collect();
        println!("{}", serde_json::to_string_pretty(&command)?);
        return Ok(0);
    }
    let title = if args.refresh {
        " Update flake inputs + rebuild "
    } else {
        match args.action {
            Action::Build => " Build current configuration ",
            Action::Test => " Build and test configuration ",
            Action::Switch => " Rebuild and activate configuration ",
            Action::Boot => " Build configuration for next boot ",
        }
    };
    // Nix records events separately from the interactive nh output.
    eprintln!(
        "\n{}",
        theme.paint(
            title,
            "on_primary_container",
            Some("primary_container"),
            styled
        )
    );
    eprintln!(
        "  nh os {} · {}\n",
        args.action.name(),
        args.flake
            .as_ref()
            .map(|s| render::safe(&s.to_string_lossy()))
            .unwrap_or_else(|| "nh environment".into())
    );
    io::stderr().flush()?;
    let start = Instant::now();
    let log = if args.observe || graphics {
        match recording(&args) {
            Ok(log) => Some(log),
            Err(error) => {
                eprintln!(
                    "  Nix recording unavailable: {}",
                    render::safe(&error.to_string())
                );
                None
            }
        }
    } else {
        None
    };
    let loader = if graphics {
        Some(kitty::Loader::start(theme, styled)?)
    } else {
        None
    };
    // Both processes receive terminal SIGINT. Keep the parent alive to wait and
    // report exit 130; the caught disposition resets to default on child exec.
    let registration = signal_hook::flag::register(
        signal_hook::consts::SIGINT,
        Arc::new(AtomicBool::new(false)),
    )?;
    let result = Command::new("nh")
        .args(arguments(&args, log.as_ref().map(|file| file.path())))
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .and_then(|mut child| child.wait());
    drop(loader);
    signal_hook::low_level::unregister(registration);
    let status = result?;
    #[cfg(unix)]
    let code = {
        use std::os::unix::process::ExitStatusExt;
        status
            .code()
            .unwrap_or_else(|| 128 + status.signal().unwrap_or(1)) as u8
    };
    #[cfg(not(unix))]
    let code = status.code().unwrap_or(1) as u8;
    if let Some(log) = log {
        match nix_events::Summary::read(log.path()) {
            Ok(summary) if summary.records > 0 => {
                summary.report();
                match log.keep() {
                    Ok((_, path)) => {
                        eprintln!("  Event log: {}", render::safe(&path.to_string_lossy()))
                    }
                    Err(error) => eprintln!("  Could not retain Nix event log: {error}"),
                }
            }
            Ok(_) => eprintln!("  No Nix events recorded for this run."),
            Err(error) => eprintln!("  Could not read Nix events: {error}"),
        }
    }
    let elapsed = start.elapsed();
    let role = if status.success() { "primary" } else { "error" };
    eprintln!(
        "\n{}",
        theme.paint(
            &format!(
                "  {} · {}m {:02}s · exit {code}",
                if status.success() {
                    "Completed"
                } else {
                    "Failed"
                },
                elapsed.as_secs() / 60,
                elapsed.as_secs() % 60
            ),
            role,
            None,
            styled
        )
    );
    Ok(code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    #[test]
    fn rebuild_does_not_update_inputs() {
        let args = Args {
            flake: Some(PathBuf::from("/tmp/a b;echo BAD").into_os_string()),
            refresh: false,
            action: Action::Build,
            dry_run: true,
            observe: false,
            replay: None,
            extra: vec!["--offline".into()],
        };
        assert_eq!(
            arguments(&args, None),
            vec!["os", "build", "/tmp/a b;echo BAD", "--offline"]
                .into_iter()
                .map(OsString::from)
                .collect::<Vec<_>>()
        );
    }
    #[test]
    fn explicit_refresh_adds_update() {
        let args = Args {
            flake: None,
            refresh: true,
            action: Action::Switch,
            dry_run: true,
            observe: false,
            replay: None,
            extra: vec![],
        };
        assert_eq!(arguments(&args, None)[2], "--update");
    }
}
