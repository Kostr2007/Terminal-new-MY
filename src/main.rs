mod browser;
mod files;
mod palette;
mod render;
mod update;

use clap::{Parser, Subcommand, ValueEnum};
use std::{
    io::{self, IsTerminal, Write},
    path::PathBuf,
    process::ExitCode,
};

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
pub enum ColorMode {
    #[default]
    Auto,
    Always,
    Never,
}

#[derive(Parser)]
#[command(
    name = "tnmy",
    version,
    about = "Terminal New MY · Material You for everyday commands"
)]
struct Cli {
    /// Material-You-Terminal palette.json (otherwise discovered automatically)
    #[arg(long, global = true, env = "TNMY_PALETTE")]
    palette: Option<PathBuf>,
    #[arg(long, global = true, value_enum, default_value = "auto")]
    color: ColorMode,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// A Material file list that stays in your scrollback
    Ls {
        #[arg(default_value = ".")]
        paths: Vec<PathBuf>,
        #[arg(short, long)]
        all: bool,
        #[arg(short, long)]
        long: bool,
        /// Emit structured entries, without terminal decoration
        #[arg(long, conflicts_with = "zero")]
        json: bool,
        /// Emit raw paths separated by NUL (Unix bytes are preserved)
        #[arg(short = '0', long)]
        zero: bool,
        #[arg(long, value_enum, default_value = "name")]
        sort: files::Sort,
    },
    /// Browse folders, filter names and preview text
    Files {
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(short, long)]
        all: bool,
    },
    /// Check palette discovery and terminal capabilities
    Doctor {
        #[arg(long)]
        json: bool,
    },
    /// Show the shared Material color roles
    Theme,
    /// Run nh with a clear operation header and a persistent result
    Update(update::Args),
}

fn run(cli: Cli) -> io::Result<u8> {
    let theme = palette::Theme::load(cli.palette.as_deref())?;
    let styled = match cli.color {
        ColorMode::Always => true,
        ColorMode::Never => false,
        ColorMode::Auto => {
            io::stdout().is_terminal()
                && std::env::var_os("NO_COLOR").is_none()
                && std::env::var("TERM").as_deref() != Ok("dumb")
        }
    };
    // Crossterm also reads NO_COLOR internally. The explicit CLI choice must
    // have the same precedence in the TUI and the scrollback renderer.
    crossterm::style::force_color_output(styled);
    match cli.command {
        Commands::Ls {
            paths,
            all,
            long,
            json,
            zero,
            sort,
        } => {
            let stdout = io::stdout();
            let mut out = stdout.lock();
            let mut failed = false;
            // Streaming output may already have started when a later path fails.
            let mut entries = Vec::new();
            for path in &paths {
                match files::read(path, all, sort) {
                    Ok(found) if json => entries.extend(found),
                    Ok(found) => {
                        render::listing(&mut out, path, &found, long, zero, styled, &theme)?
                    }
                    Err(e) => {
                        eprintln!("tnmy: {}: {e}", render::safe(&path.to_string_lossy()));
                        failed = true;
                    }
                }
            }
            if json {
                let data = serde_json::to_string_pretty(&entries).map_err(|e| {
                    io::Error::other(format!("{e}; use --zero for non-UTF-8 paths"))
                })?;
                writeln!(out, "{data}")?;
            }
            Ok(u8::from(failed))
        }
        Commands::Files { path, all } => {
            if !io::stdin().is_terminal()
                || !io::stdout().is_terminal()
                || std::env::var("TERM").as_deref() == Ok("dumb")
            {
                return Err(io::Error::other(
                    "files needs an interactive terminal; use tnmy ls for redirected output",
                ));
            }
            browser::run(path, all, theme, styled)?;
            Ok(0)
        }
        Commands::Doctor { json } => {
            let report = serde_json::json!({
                "version": env!("CARGO_PKG_VERSION"),
                "palette": theme.source, "palette_warning": theme.warning,
                "mode": theme.mode, "stdout_tty": io::stdout().is_terminal(),
                "color": styled, "kitty": std::env::var_os("KITTY_WINDOW_ID").is_some(),
                "term": std::env::var("TERM").unwrap_or_default(),
                "nh": update::available(),
            });
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                render::doctor(&report, styled, &theme)?;
            }
            Ok(0)
        }
        Commands::Theme => {
            render::theme(&theme, styled)?;
            Ok(0)
        }
        Commands::Update(args) => update::run(args, styled, &theme),
    }
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(code) => ExitCode::from(code),
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("tnmy: {}", render::safe(&e.to_string()));
            ExitCode::FAILURE
        }
    }
}
