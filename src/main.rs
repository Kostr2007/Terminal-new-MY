mod browser;
mod files;
mod git;
mod palette;
mod preview;
mod render;
mod tool_themes;
mod ui;
mod update;
mod worker;

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
    /// Interface density for lists and the file browser
    #[arg(
        long,
        global = true,
        value_enum,
        default_value = "comfortable",
        env = "TNMY_DENSITY"
    )]
    density: ui::Density,
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
        /// Include bounded Git status metadata
        #[arg(long)]
        git: bool,
    },
    /// Browse folders, filter names and preview text
    Files {
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(short, long)]
        all: bool,
        /// Disable Git subprocesses
        #[arg(long)]
        no_git: bool,
        /// Print the chosen path as NUL-terminated bytes; UI uses stderr
        #[arg(long, conflicts_with = "pick_directory")]
        pick: bool,
        /// Choose a directory for shell integration
        #[arg(long)]
        pick_directory: bool,
    },
    /// Check palette discovery and terminal capabilities
    Doctor {
        #[arg(long)]
        json: bool,
    },
    /// Show the shared Material color roles
    Theme(tool_themes::Options),
    /// Run nh with a clear operation header and a persistent result
    Update(update::Args),
}

fn run(cli: Cli) -> io::Result<u8> {
    let theme = palette::Theme::load(cli.palette.as_deref())?;
    let picker = matches!(
        &cli.command,
        Commands::Files { pick: true, .. }
            | Commands::Files {
                pick_directory: true,
                ..
            }
    );
    let styled = match cli.color {
        ColorMode::Always => true,
        ColorMode::Never => false,
        ColorMode::Auto => {
            (if picker {
                io::stderr().is_terminal()
            } else {
                io::stdout().is_terminal()
            }) && std::env::var_os("NO_COLOR").is_none()
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
            git,
        } => {
            let mut git_cache = git::Cache::default();
            let stdout = io::stdout();
            let mut out = stdout.lock();
            let mut failed = false;
            // Streaming output may already have started when a later path fails.
            let mut entries = Vec::new();
            for path in &paths {
                let result = files::read(path, all, sort).map(|mut entries| {
                    if git {
                        let directory = if path.is_dir() {
                            path.as_path()
                        } else {
                            path.parent().unwrap_or(std::path::Path::new("."))
                        };
                        if let Ok(directory) = std::fs::canonicalize(directory) {
                            let snapshot =
                                git_cache.scan(&directory, false, &worker::Cancel::fresh());
                            for entry in &mut entries {
                                let full =
                                    directory.join(entry.path.file_name().unwrap_or_default());
                                let label = snapshot.label(&full);
                                if label != "  " {
                                    entry.git_status = Some(label.into());
                                }
                            }
                        }
                    }
                    entries
                });
                match result {
                    Ok(found) if json => entries.extend(found),
                    Ok(found) => render::listing(
                        &mut out,
                        path,
                        &found,
                        render::ListOptions {
                            long,
                            zero,
                            styled,
                            density: cli.density,
                        },
                        &theme,
                    )?,
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
        Commands::Files {
            path,
            all,
            no_git,
            pick,
            pick_directory,
        } => {
            if !io::stdin().is_terminal()
                || !io::stderr().is_terminal()
                || (!picker && !io::stdout().is_terminal())
                || std::env::var("TERM").as_deref() == Ok("dumb")
            {
                return Err(io::Error::other(
                    "files needs an interactive terminal; use tnmy ls for redirected output",
                ));
            }
            let mode = if pick {
                browser::Pick::Path
            } else if pick_directory {
                browser::Pick::Directory
            } else {
                browser::Pick::None
            };
            let selected = browser::run(
                browser::Options {
                    path,
                    all,
                    density: cli.density,
                    git: !no_git,
                    pick: mode,
                },
                theme,
                styled,
            )?;
            if let Some(path) = selected {
                let mut out = io::stdout().lock();
                #[cfg(unix)]
                {
                    use std::os::unix::ffi::OsStrExt;
                    out.write_all(path.as_os_str().as_bytes())?;
                }
                #[cfg(not(unix))]
                out.write_all(path.to_string_lossy().as_bytes())?;
                out.write_all(&[0])?;
            } else if picker {
                return Ok(130);
            }
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
        Commands::Theme(options) => {
            if let Some(path) = &options.export {
                tool_themes::export(&theme, path, options.build_bat)?;
            }
            if options.check_contrast {
                let report = tool_themes::contrasts(&theme);
                if options.json {
                    println!("{}", serde_json::to_string_pretty(&report)?);
                } else {
                    for pair in &report {
                        println!(
                            "{} / {}: {:.2}:1 → {:.2}:1{}",
                            pair.foreground,
                            pair.background,
                            pair.original,
                            pair.effective,
                            if pair.adjusted { " (adjusted)" } else { "" }
                        );
                    }
                }
                return Ok(u8::from(report.iter().any(|p| p.effective < 4.5)));
            }
            if options.export.is_none() {
                render::theme(&theme, styled)?;
            }
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
