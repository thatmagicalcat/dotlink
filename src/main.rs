use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::process::exit;

use clap::{Parser, Subcommand};
use colored::Colorize;
use glob::glob;
use path_clean::PathClean;
use serde::Deserialize;
use serde::Serialize;

const CFG_FILE_ENV_VAR: &str = "DOTLINK_ROOT";
const CFG_FILE: &str = "Link.toml";

fn main() -> io::Result<()> {
    let cli = Cli::parse();
    let cfg_path = get_cfg_path(&cli)?;
    let cfg = load_cfg(&cfg_path)?;

    match cli.commands {
        Commands::Link { entry } => link(&cfg, &entry)?,
        Commands::Validate => validate(&cfg)?,
        Commands::Add { targets, root } => add(cfg_path, cfg, targets, root)?,
    }

    Ok(())
}

#[derive(Parser)]
#[command(long_about = None)]
struct Cli {
    /// Config path (finds one in the current directory if not specified)
    #[clap(short)]
    config: Option<PathBuf>,

    #[command(subcommand)]
    commands: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Link entries
    Link {
        /// The directory or file entry to link
        entry: String,
    },

    /// Add the specified file or directory to dotfiles_root
    Add {
        targets: Vec<PathBuf>,
        /// Use a custom root, uses DOTLINK_ROOT env variable if not specified
        #[clap(long)]
        root: Option<PathBuf>,
    },

    /// Validate config entries and symlinks
    Validate,
}

#[derive(Debug, Serialize, Deserialize)]
struct Config {
    settings: Settings,
    #[serde(default)]
    entries: HashMap<PathBuf, PathBuf>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Settings {
    dotlink_root: Option<PathBuf>,
}

impl Config {
    fn get_root(&self) -> io::Result<PathBuf> {
        fs::canonicalize(
            &self
                .settings
                .dotlink_root
                .clone()
                .unwrap_or_else(|| PathBuf::from(
                    std::env::var(CFG_FILE_ENV_VAR).expect(
                        &format!("specify `dotfiles_root` in configuration file or `{CFG_FILE_ENV_VAR}` environment variable."))
                    )
                ),
        )
    }

    fn entries(&self) -> io::Result<impl Iterator<Item = (PathBuf, PathBuf, PathBuf)>> {
        let base = self.get_root()?;
        Ok(self.entries.iter().map(move |(source, target)| {
            (source.clean(), base.join(source.clean()), target.clean())
        }))
    }
}

fn link(cfg: &Config, entry: &str) -> io::Result<()> {
    let entry = PathBuf::from(entry).clean();
    let entries = cfg
        .entries()?
        .map(|(_, i, t)| (i.file_name().unwrap().to_str().unwrap().to_string(), (i, t))) // wtf?
        .collect::<Vec<_>>();

    // validate
    let Some((_, (actual_path, symlink_target))) = entries
        .iter()
        .find(|src| entry.to_str() == Some(src.0.as_str()))
    else {
        eprintln!("Entry {entry:?} not found in config");
        eprintln!("Valid entries are:");

        entries.iter().for_each(|(name, _)| eprintln!("\t{name}"));
        return Ok(());
    };

    let symlink_target = expand_tilde(symlink_target);
    if fs::exists(&symlink_target)? {
        println!("[Info] Symlink target {symlink_target:?} already exists");
        return Ok(());
    }

    std::os::unix::fs::symlink(actual_path, symlink_target)
}

fn expand_tilde(path: &PathBuf) -> String {
    path.to_str()
        .unwrap()
        .to_string()
        .replace("~", &std::env::var("HOME").expect("Cannot expand ~"))
}

fn check_symlinks(cfg: &Config) -> io::Result<()> {
    for (specified, source, target) in cfg.entries()? {
        if !fs::exists(&source)? {
            eprintln!("[WARN] File {source:?} does not exists.");
            continue;
        }

        match fs::symlink_metadata(&target) {
            Ok(entry) if entry.is_symlink() => println!("󰄬 {specified:?} -> {target:?}",),
            Ok(_) => println!("{target:?} Exists but it is not a symlink"),

            Err(e) if matches!(e.kind(), io::ErrorKind::NotFound) => {
                println!("󰜺 {specified:?} -> {target:?}")
            }

            Err(e) => eprintln!("Error: {e:?}"),
        }
    }

    Ok(())
}

fn get_cfg_path(cli: &Cli) -> io::Result<PathBuf> {
    let cwd = std::env::current_dir()?;
    let mut cfg_path = cli.config.clone().unwrap_or(cwd.join(CFG_FILE));

    if !fs::exists(&cfg_path)? {
        if let Ok(var) = std::env::var(CFG_FILE_ENV_VAR) {
            let alt = PathBuf::from(var).join(CFG_FILE);
            if fs::exists(&alt)? {
                cfg_path = alt;
            } else {
                eprintln!("Config not found at {cfg_path:?} or {alt:?}");
                exit(1);
            }
        } else {
            eprintln!("Config not found at {cfg_path:?} and no {CFG_FILE_ENV_VAR} set.");
            exit(1);
        }
    }

    Ok(cfg_path)
}

fn link_all(cfg: &Config) -> io::Result<()> {
    todo!();
}

fn load_cfg(cfg_path: &Path) -> Result<Config, io::Error> {
    let cfg_contents = fs::read_to_string(cfg_path)?;
    let cfg = toml::from_str::<Config>(&cfg_contents).unwrap_or_else(|e| {
        eprintln!("Failed to parse config file: {e}");
        exit(0);
    });

    Ok(cfg)
}

fn add_one(
    cfg: &mut Config,
    cfg_path: &PathBuf,
    target: PathBuf,
    root: Option<PathBuf>,
) -> io::Result<()> {
    let root = match root {
        Some(k) if fs::exists(&k)? => k,
        Some(k) => {
            eprintln!("Path `{k:?}` does not exist");
            return Ok(());
        }

        _ => match std::env::var(CFG_FILE_ENV_VAR) {
            Ok(k) if fs::exists(&k)? => k.into(),
            Ok(k) => {
                eprintln!("Path `{k}` does not exist");
                return Ok(());
            }

            _ => {
                eprintln!("Failed to determine dotlink root directory");
                return Ok(());
            }
        },
    };

    if !fs::exists(&target)? {
        eprintln!("Target: {target:?} does not exists");
    }

    let target = target.canonicalize()?.clean();

    if cfg
        .entries()?
        .map(|(_, i, _)| i.file_name().unwrap().to_str().unwrap().to_string())
        .any(|i| i == target.file_name().unwrap().to_str().unwrap())
    {
        eprintln!("Target entry `{target:?}` already exists in config.");
        return Ok(());
    }

    move_dir(target.to_str().unwrap(), root.to_str().unwrap())?;

    let name = target
        .components()
        .last()
        .expect("Target path has no components");

    cfg.entries.insert(root.join(name), target.clone());
    link(cfg, name.as_os_str().to_str().unwrap())?;

    fs::write(
        cfg_path,
        toml::to_string_pretty(cfg).expect("failed to serialize config"),
    )
}

fn move_dir(src: &str, dst: &str) -> io::Result<()> {
    std::process::Command::new("mv")
        .args(&[src, dst])
        .spawn()?
        .wait()
        .map(|_| ())
}

fn validate(cfg: &Config) -> io::Result<()> {
    let mut all_ok = true;

    for (name, source, target) in cfg.entries()? {
        let target_path = expand_tilde(&target);
        let target_path = PathBuf::from(target_path);

        // Check source
        if !source.exists() {
            eprintln!("✖ Source missing: {source:?}");
            all_ok = false;
            continue;
        }

        // Check target
        if !target_path.exists() {
            println!(
                "{}",
                format!("󰜺 Missing target: {target:?} — will be created").blue()
            );
        } else {
            let meta = fs::symlink_metadata(&target_path)?;
            if meta.file_type().is_symlink() {
                let actual = fs::read_link(&target_path)?;
                if actual != source {
                    eprintln!(
                        "⚠ Symlink mismatch: {target:?} points to {actual:?}, expected {source:?}"
                    );
                    all_ok = false;
                } else {
                    println!(
                        "{}",
                        format!("󰄬 {name:?} -> {target:?} [ok]").white().bold()
                    );
                }
            } else {
                eprintln!("✖ Conflict: {target:?} exists and is not a symlink");
                all_ok = false;
            }
        }
    }

    if all_ok {
        println!("✅ All entries validated successfully");
    } else {
        println!("❌ Some issues were found.");
    }

    Ok(())
}

fn resolve_targets(pattern: &str) -> io::Result<Vec<PathBuf>> {
    Ok(glob(pattern)
        .expect("Fsiled to read glob pattern")
        .filter_map(|i| {
            i.inspect_err(|e| eprintln!("{} {}", "Glob error:".red(), e.to_string().red()))
                .ok()
        })
        .collect::<Vec<_>>())
}

fn add(
    cfg_path: PathBuf,
    mut cfg: Config,
    targets: Vec<PathBuf>,
    root: Option<PathBuf>,
) -> io::Result<()> {
    Ok(for pattern in targets {
        for path in resolve_targets(pattern.to_str().unwrap())? {
            println!(
                "[{}] {} {}",
                "INFO".yellow(),
                "adding".yellow(),
                format!("{path:?}").yellow()
            );

            add_one(&mut cfg, &cfg_path, path, root.clone())?;
        }
    })
}
