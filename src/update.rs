use crate::{palette::Theme, render};
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

fn arguments(args: &Args) -> Vec<OsString> {
    let mut command = vec!["os".into(), args.action.name().into()];
    if args.refresh {
        command.push("--update".into());
    }
    if let Some(flake) = &args.flake {
        command.push(flake.clone());
    }
    command.extend(args.extra.iter().cloned());
    command
}

pub fn run(args: Args, styled: bool, theme: &Theme) -> io::Result<u8> {
    let arguments = arguments(&args);
    if args.dry_run {
        // A JSON argument vector is unambiguous even when paths contain shell syntax.
        let command: Vec<_> = std::iter::once(OsString::from("nh"))
            .chain(arguments)
            .map(|s| s.to_string_lossy().into_owned())
            .collect();
        println!("{}", serde_json::to_string_pretty(&command)?);
        return Ok(0);
    }
    let title = if args.refresh {
        " Update flake inputs + rebuild "
    } else {
        " Rebuild current configuration "
    };
    // nh retains its own terminal, progress display, password prompts and activation logic.
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
    // Both processes receive terminal SIGINT. Keep the parent alive to wait and
    // report exit 130; the caught disposition resets to default on child exec.
    let registration = signal_hook::flag::register(
        signal_hook::consts::SIGINT,
        Arc::new(AtomicBool::new(false)),
    )?;
    let result = Command::new("nh")
        .args(arguments)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .and_then(|mut child| child.wait());
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
            extra: vec!["--offline".into()],
        };
        assert_eq!(
            arguments(&args),
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
            extra: vec![],
        };
        assert_eq!(arguments(&args)[2], "--update");
    }
}
