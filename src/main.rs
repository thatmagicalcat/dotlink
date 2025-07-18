use std::collections::HashMap;
use std::collections::HashSet;
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
    let mut cfg = load_cfg(&cfg_path)?;

    match cli.commands {
        Commands::Fix => fix(&cfg)?,
        Commands::Add { targets, root } => add(cfg_path, &mut cfg, &targets, root)?,
        Commands::Unlink { entries } => unlink(cfg_path, &mut cfg, &entries)?,
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
    /// Create missing entries and validates existing ones
    Fix,

    /// Unlink entries
    Unlink { entries: Vec<String> },

    /// Add the specified file or directory to dotfiles_root
    Add {
        targets: Vec<String>,
        /// Use a custom root, uses DOTLINK_ROOT env variable if not specified
        #[clap(long)]
        root: Option<PathBuf>,
    },
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


fn expand_tilde(path: &PathBuf) -> String {
    path.to_str()
        .unwrap()
        .to_string()
        .replace("~", &std::env::var("HOME").expect("Cannot expand ~"))
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
    root: &Path,
) -> io::Result<()> {
    if !target.exists() {
        eprintln!("Target: {:?} does not exist", target);
        return Ok(());
    }

    let target = target.canonicalize()?.clean();
    let name = target.file_name().unwrap_or_else(|| {
        eprintln!("Fatal: Could not determine filename for target {:?}", target);
        exit(1);
    });

    let dest_in_root = root.join(name);

    // check if an entry with the same destination path already exists
    if cfg.entries.contains_key(&dest_in_root) {
        eprintln!(
            "Target entry for {:?} already exists in config.",
            dest_in_root
        );
        return Ok(());
    }

    // move the original file/dir into the dotfiles root
    println!(
        "  - Moving {} -> {}",
        format!("{:?}", target.display()).cyan(),
        format!("{:?}", dest_in_root.display()).cyan()
    );

    fs::rename(&target, &dest_in_root)?;

    cfg.entries.insert(dest_in_root.clone(), target.clone());

    let actual_path = &dest_in_root;
    let symlink_target = &target; // `target` is already canonicalized and absolute

    if symlink_target.exists() || fs::symlink_metadata(symlink_target).is_ok() {
        println!(
            "[{}] Symlink target {:?} already exists, skipping.",
            "Info".yellow(),
            symlink_target
        );
    } else {
        if let Some(parent) = symlink_target.parent() {
            fs::create_dir_all(parent)?;
        }

        println!(
            "  - Linking {} -> {}",
            format!("{:?}", actual_path.display()).cyan(),
            format!("{:?}", symlink_target.display()).cyan()
        );

        std::os::unix::fs::symlink(actual_path, symlink_target)?;
    }

    fs::write(
        cfg_path,
        toml::to_string_pretty(cfg).expect("failed to serialize config"),
    )
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
        .expect("Failed to read glob pattern")
        .filter_map(|i| {
            i.inspect_err(|e| eprintln!("{} {}", "Glob error:".red(), e.to_string().red()))
                .ok()
        })
        .collect::<Vec<_>>())
}

fn add(
    cfg_path: PathBuf,
    cfg: &mut Config,
    targets: &[String],
    root: Option<PathBuf>,
) -> io::Result<()> {
    let dotlink_root = match root {
        Some(r) => r,
        None => cfg.get_root()?,
    };

    if !dotlink_root.exists() {
        eprintln!(
            "{} Dotfiles root directory `{:?}` does not exist.",
            "Error:".red(),
            dotlink_root
        );

        exit(1);
    }

    for pattern in targets {
        for path in resolve_targets(pattern)? {
            println!(
                "[{}] adding {}",
                "INFO".yellow(),
                format!("{:?}", path.display()).bold()
            );

            add_one(cfg, &cfg_path, path, &dotlink_root)?;
        }
    }

    Ok(())
}

fn unlink(cfg_path: PathBuf, cfg: &mut Config, entries: &[String]) -> io::Result<()> {
    let mut targets_to_process = HashSet::new();
    for pattern in entries {
        for path in resolve_targets(pattern)? {
            match fs::canonicalize(&path) {
                Ok(canon_path) => {
                    targets_to_process.insert(canon_path);
                }

                Err(_) => {
                    targets_to_process.insert(path.clean());
                }
            }
        }
    }

    if targets_to_process.is_empty() {
        println!("No valid targets found to unlink.");
        return Ok(());
    }

    let mut keys_to_remove = Vec::new();
    let mut changed = false;

    for (source_path_abs, target_path_unexpanded) in &cfg.entries {
        let target_path_abs = PathBuf::from(expand_tilde(target_path_unexpanded)).clean();

        // Check if either the source (in dotfiles_root) or the target (symlink)
        // was specified by the user.
        if targets_to_process.contains(source_path_abs)
            || targets_to_process.contains(&target_path_abs)
        {
            println!(
                "[{}] Unlinking {}",
                "INFO".yellow(),
                format!("{:?}", source_path_abs.file_name().unwrap()).bold()
            );

            // remove the symlink.
            // Use `symlink_metadata` to check the path without following the link
            if let Ok(metadata) = fs::symlink_metadata(&target_path_abs) {
                if metadata.file_type().is_symlink() {
                    println!(
                        "  - Removing symlink at {}",
                        format!("{:?}", target_path_abs.display()).cyan()
                    );
                    fs::remove_file(&target_path_abs)?;
                } else {
                    eprintln!(
                        "  {} Path at {:?} is not a symlink, but is the target for this entry. Please resolve manually.",
                        "Warning:".yellow(),
                        target_path_abs.display()
                    );
                }
            }

            // move the file/dir from dotfiles_root back to the target location
            if source_path_abs.exists() {
                println!(
                    "  - Moving {} -> {}",
                    format!("{:?}", source_path_abs.display()).cyan(),
                    format!("{:?}", target_path_abs.display()).cyan()
                );

                // move
                fs::rename(source_path_abs, &target_path_abs)?;
            } else {
                eprintln!(
                    "  {} Source file {:?} does not exist in dotfiles root. Cannot move it.",
                    "Warning:".yellow(),
                    source_path_abs.display()
                );
            }

            // mark this entry's key for removal from the config.
            keys_to_remove.push(source_path_abs.clone());
            changed = true;
        }
    }

    // update the config if changes were made
    if changed {
        println!("[{}] Updating config file...", "INFO".yellow());
        for key in keys_to_remove {
            cfg.entries.remove(&key);
        }

        fs::write(
            &cfg_path,
            toml::to_string_pretty(cfg).expect("Failed to serialize config"),
        )?;
        println!("✅ Unlink operation complete.");
    } else {
        println!("No matching entries found in config for the given paths.");
    }

    Ok(())
}

fn fix(cfg: &Config) -> io::Result<()> {
    println!("[{}] Checking and fixing links...", "INFO".yellow());
    let mut all_ok = true;

    for (name, source, target) in cfg.entries()? {
        let target_path = PathBuf::from(expand_tilde(&target));
        let name_os_str = name.file_name().unwrap_or(name.as_os_str());

        if !source.exists() {
            eprintln!("✖ Source missing for {:?}: {:?}", name_os_str, source);
            all_ok = false;
            continue;
        }

        match fs::symlink_metadata(&target_path) {
            Ok(metadata) => {
                // Target path exists.
                if metadata.file_type().is_symlink() {
                    // It's a symlink, check if it points to the correct source.
                    let actual_link_target = fs::read_link(&target_path)?;
                    if actual_link_target != source {
                        eprintln!(
                            "⚠ Symlink mismatch for {:?}: {:?} points to {:?}, expected {:?}",
                            name_os_str, target, actual_link_target, source
                        );
                        all_ok = false;
                    } else {
                        println!(
                            "{}",
                            format!("󰄬 {:?} -> {:?} [ok]", name_os_str, target.display())
                                .white()
                                .bold()
                        );
                    }
                } else {
                    // it's a file or directory, not a symlink. This is a conflict
                    eprintln!("✖ Conflict: {:?} exists and is not a symlink.", target);
                    all_ok = false;
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                // target path does not exist. This is where we "fix" it
                println!(
                    "{}",
                    format!(
                        "󰜺 Missing link for {:?}: {:?} -> {:?}. Creating...",
                        name_os_str,
                        source.file_name().unwrap(),
                        target.display()
                    )
                    .blue()
                );

                // ensure parent directory exists before creating symlink
                if let Some(parent) = target_path.parent() {
                    fs::create_dir_all(parent)?;
                }

                // create the symlink
                std::os::unix::fs::symlink(&source, &target_path)?;
                println!(
                    "  {}",
                    format!("Successfully created link for {:?}", name_os_str).green()
                );
            }
            Err(e) => {
                eprintln!("✖ Error checking path {:?}: {}", target_path, e);
                all_ok = false;
            }
        }
    }

    if all_ok {
        println!("\n✅ All links are correct.");
    } else {
        println!("\n❌ Some issues were found.");
    }

    Ok(())
}
