//! bvm — the Bun version manager. One binary: run as `bvm` it manages
//! versions; run as `bun` or `bunx` (the shims `bvm setup` places on PATH) it
//! starts the Bun version selected for the current directory.

mod adopt;
mod channel;
mod install;
mod paths;
mod platform;
mod resolve;
mod run;
mod selfmanage;
mod setup;
mod ui;
mod verify;

use anyhow::{Context, Result, anyhow};
use channel::{Channel, Version};
use clap::builder::styling::{AnsiColor, Styles};
use clap::{Parser, Subcommand};
use std::ffi::OsString;
use std::fs;
use std::path::Path;

const HELP_STYLES: Styles = Styles::styled()
    .header(AnsiColor::Green.on_default().bold())
    .usage(AnsiColor::Green.on_default().bold())
    .literal(AnsiColor::Cyan.on_default().bold())
    .placeholder(AnsiColor::Cyan.on_default())
    .error(AnsiColor::Red.on_default().bold())
    .valid(AnsiColor::Cyan.on_default().bold())
    .invalid(AnsiColor::Yellow.on_default().bold());

#[derive(Parser)]
#[command(
    name = "bvm",
    version,
    about = "Install, switch and verify Bun versions",
    disable_version_flag = true,
    styles = HELP_STYLES
)]
struct Cli {
    /// Print the version (`-v`, `-V` or `--version`, as `bun -v` does).
    #[arg(short = 'v', long, short_alias = 'V', action = clap::ArgAction::Version)]
    version: Option<bool>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Install a version: `1.4.2`, `latest`, `1.4.2-absolute.1`, or `absolute`.
    Install {
        version: String,
        /// Also make it the default.
        #[arg(long)]
        default: bool,
    },
    /// Remove an installed version.
    Uninstall { version: String },
    /// Use a version in this shell (needs the shell function from `bvm setup`).
    Use {
        version: String,
        #[arg(long, hide = true)]
        print_env: bool,
        #[arg(long, default_value = "sh", hide = true)]
        shell: String,
    },
    /// Set the version used everywhere nothing else selects one.
    Default { version: String },
    /// List installed versions.
    Ls,
    /// List versions available to install.
    LsRemote {
        /// AbsoluteJS's patched builds instead of official Bun.
        #[arg(long)]
        absolute: bool,
    },
    /// Show the version selected here, and why.
    Current,
    /// Print the path of the Bun binary selected here (or of a given version).
    Which { version: Option<String> },
    /// Run a command with a given version: `bvm exec 1.4.2 -- bun --version`.
    Exec {
        version: String,
        #[arg(last = true, required = true)]
        command: Vec<OsString>,
    },
    /// Print the shell setup (`eval "$(bvm env)"`).
    Env {
        #[arg(long, default_value = "sh")]
        shell: String,
    },
    /// Install the shims and add them to your shell's PATH.
    Setup,
    /// Manage bvm itself.
    #[command(name = "self")]
    Bvm {
        #[command(subcommand)]
        action: SelfAction,
    },
}

#[derive(Subcommand)]
enum SelfAction {
    /// Replace bvm with the latest signed release.
    Update,
}

fn installed_version(text: &str) -> Result<Version> {
    let version = if text == "latest" || text == "absolute" {
        let channel = if text == "latest" {
            Channel::Official
        } else {
            Channel::Absolute
        };
        install::installed()?
            .into_iter()
            .rfind(|v| v.channel() == channel)
            .ok_or_else(|| anyhow!("no installed {text} version"))?
    } else {
        Version::parse(text)?
    };
    if !install::is_installed(&version)? {
        return Err(anyhow!(
            "Bun {version} is not installed; run `bvm install {version}`"
        ));
    }
    Ok(version)
}

/// A Bun installed before bvm: install the same version through bvm, verified,
/// and make it the default, so `bun` keeps meaning what it meant. Skipped once
/// bvm manages any version.
fn adopt_existing_bun() {
    if !install::installed().map(|v| v.is_empty()).unwrap_or(false) {
        return;
    }
    let Some(existing) = adopt::find() else {
        return;
    };
    let paint = ui::err();
    ui::done(format!(
        "Found Bun {} at {}",
        paint.version(&existing.version),
        paint.path(&existing.path)
    ));
    let adopted = install::install(&existing.version).and_then(|()| {
        if resolve::default_version()?.is_none() {
            fs::write(paths::default_file()?, existing.version.to_string())?;
        }
        Ok(())
    });
    match adopted {
        Ok(()) => ui::done(format!(
            "Bun {} is your default; projects can pin others",
            paint.version(&existing.version)
        )),
        Err(error) => ui::warn(format!(
            "could not install Bun {} through bvm ({error:#}); run `bvm install {} --default` later",
            existing.version, existing.version
        )),
    }
}

fn run_shim(name: &str, args: Vec<OsString>) -> Result<i32> {
    let cwd = std::env::current_dir()?;
    let resolved = resolve::resolve(&cwd)?;
    let version = resolved.version;
    if !install::is_installed(&version)? {
        // An exact version the project pins: install it (fully verified)
        // rather than fail, as Volta does. BVM_AUTO_INSTALL=0 turns this off.
        if std::env::var("BVM_AUTO_INSTALL").as_deref() == Ok("0") {
            return Err(anyhow!(
                "Bun {version} (from {}) is not installed; run `bvm install {version}`",
                resolved.source
            ));
        }
        ui::working(format!(
            "Bun {} (from {}) is not installed yet; installing it",
            ui::err().version(&version),
            resolved.source
        ));
        install::install(&version)?;
    }
    let binary = paths::bun_binary(&version)?;
    let args = if name == "bunx" {
        std::iter::once(OsString::from("x")).chain(args).collect()
    } else {
        args
    };
    run::exec(&binary, args)
}

fn run_cli() -> Result<i32> {
    let cli = Cli::parse();
    match cli.command {
        Command::Install { version, default } => {
            let version = channel::resolve_request(&version)?;
            install::install(&version)?;
            if default {
                fs::create_dir_all(paths::root()?)?;
                fs::write(paths::default_file()?, version.to_string())?;
                ui::done(format!(
                    "Default is now Bun {}",
                    ui::err().version(&version)
                ));
            }
        }
        Command::Uninstall { version } => install::uninstall(&Version::parse(&version)?)?,
        Command::Use {
            version,
            print_env,
            shell,
        } => {
            let version = installed_version(&version)?;
            if print_env {
                println!("{}", setup::use_env(&shell, &version.to_string()));
                ui::done(format!(
                    "Using Bun {} in this shell",
                    ui::err().version(&version)
                ));
            } else {
                ui::error(
                    "`bvm use` needs the shell function from `bvm setup` (or `eval \"$(bvm env)\"`)",
                );
                ui::note(format!("For one command: `bvm exec {version} -- bun ...`"));
                return Ok(1);
            }
        }
        Command::Default { version } => {
            let version = installed_version(&version)?;
            fs::write(paths::default_file()?, version.to_string())?;
            ui::done(format!(
                "Default is now Bun {}",
                ui::err().version(&version)
            ));
        }
        Command::Ls => {
            let default = resolve::default_version()?;
            let current = resolve::resolve(&std::env::current_dir()?)
                .ok()
                .map(|r| r.version);
            let installed = install::installed()?;
            if installed.is_empty() {
                ui::note("No Bun versions installed yet. Run `bvm install latest --default`");
            }
            let paint = ui::out();
            let width = installed
                .iter()
                .map(|version| version.to_string().len())
                .max()
                .unwrap_or(0);
            for version in installed {
                let is_current = current.as_ref() == Some(&version);
                let marks = [
                    is_current.then_some("current"),
                    (default.as_ref() == Some(&version)).then_some("default"),
                ];
                let marks: Vec<&str> = marks.into_iter().flatten().collect();
                let name = format!("{:width$}", version.to_string());
                let line = format!(
                    "{} {}  {}",
                    if is_current {
                        paint.green("*")
                    } else {
                        " ".into()
                    },
                    if is_current { paint.green(&name) } else { name },
                    paint.dim(marks.join(", "))
                );
                println!("{}", line.trim_end());
            }
        }
        Command::LsRemote { absolute } => {
            let channel = if absolute {
                Channel::Absolute
            } else {
                Channel::Official
            };
            let installed = install::installed()?;
            let paint = ui::out();
            for version in channel::remote_versions(channel)? {
                if installed.contains(&version) {
                    println!("{}  {}", paint.green(&version), paint.dim("installed"));
                } else {
                    println!("{version}");
                }
            }
        }
        Command::Current => {
            let resolved = resolve::resolve(&std::env::current_dir()?)?;
            let paint = ui::out();
            println!(
                "{}  {}",
                paint.version(&resolved.version),
                paint.dim(format!("(from {})", resolved.source))
            );
        }
        Command::Which { version } => {
            let version = match version {
                Some(text) => installed_version(&text)?,
                None => resolve::resolve(&std::env::current_dir()?)?.version,
            };
            println!("{}", paths::bun_binary(&version)?.display());
        }
        Command::Exec { version, command } => {
            let version = installed_version(&version)?;
            let (program, args) = command
                .split_first()
                .ok_or_else(|| anyhow!("no command given"))?;
            let mut child = std::process::Command::new(program);
            child.args(args).env("BVM_BUN_VERSION", version.to_string());
            let bin = paths::bin_dir()?;
            let path = std::env::var_os("PATH").unwrap_or_default();
            let mut entries = vec![bin];
            entries.extend(std::env::split_paths(&path));
            child.env("PATH", std::env::join_paths(entries)?);
            return Ok(child.status()?.code().unwrap_or(1));
        }
        Command::Env { shell } => {
            let text = match shell.as_str() {
                "powershell" | "pwsh" => setup::powershell_env()?,
                "fish" => setup::fish_env()?,
                _ => setup::posix_env()?,
            };
            print!("{text}");
        }
        Command::Setup => {
            let me = std::env::current_exe().context("cannot locate the bvm binary")?;
            selfmanage::place_shims(&me)?;
            let changed = setup::setup()?;
            let paint = ui::err();
            ui::done(format!(
                "Shims installed in {}",
                paint.path(&paths::bin_dir()?)
            ));
            for file in &changed {
                ui::done(format!("Added bvm to {}", paint.path(file)));
            }
            adopt_existing_bun();
            // The installers print their own closing advice.
            if std::env::var_os("BVM_FROM_INSTALLER").is_none() {
                let bvm = paths::bin_dir()?.join(format!("bvm{}", platform::exe_suffix()));
                let activate = if cfg!(windows) {
                    format!(
                        "Invoke-Expression (& \"{}\" env --shell powershell | Out-String)",
                        bvm.display()
                    )
                } else {
                    format!("eval \"$(\"{}\" env)\"", bvm.display())
                };
                ui::say("");
                ui::say("  New terminals are set up. For this one, run:");
                ui::say("");
                ui::say(format!("    {}", paint.cmd(activate)));
                ui::say("");
            }
        }
        Command::Bvm {
            action: SelfAction::Update,
        } => selfmanage::update()?,
    }
    Ok(0)
}

fn main() {
    let mut args = std::env::args_os();
    let argv0 = args.next().unwrap_or_default();
    let name = Path::new(&argv0)
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let result = match name.as_str() {
        "bun" | "bunx" => run_shim(&name, args.collect()),
        _ => run_cli(),
    };
    match result {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            ui::error(format!("{error:#}"));
            std::process::exit(1);
        }
    }
}
