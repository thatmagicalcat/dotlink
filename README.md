# Dotlink

A simple, command-line tool for managing your dotfiles. Dotlink helps you keep your configuration files synchronized by storing them in a central directory and creating symbolic links to their original locations.

## How it Works

Dotlink operates on a central concept:

1. You have a dedicated dotfiles root directory (e.g., ~/dotfiles) where the actual configuration files are stored.
2. Your configuration file, Link.toml, keeps a record of which files are managed and where their corresponding symlinks should be.
3. Dotlink creates symbolic links from the locations where applications expect to find their configs (e.g., ~/.config/nvim/init.vim) to the actual files in your dotfiles root.

This allows you to version control your dotfiles directory while keeping everything in its right place.

## Configuration

Dotlink is configured through a Link.toml file and an environment variable.
`Link.toml`

This file is the heart of your configuration. It tells Dotlink where your dotfiles root is and which files to manage. It should be placed in your dotfiles root directory.

```toml
# `Link.toml

[settings]
# (Optional) You can specify the root directory here.
# If not set, the DOTLINK_ROOT environment variable MUST be set.
# dotlink_root = "/home/user/dotfiles"

[entries]
# This section maps the actual file in your dotfiles root
# to the location where the symlink should be created.
#
# Format:
# "/absolute/path/to/file/in/dotfiles/root" = "/path/to/symlink/location"
#
# Note: The key MUST be an absolute path.
"/home/user/dotfiles/.bashrc" = "~/.bashrc"
"/home/user/dotfiles/nvim" = "~/.config/nvim"
```

`DOTLINK_ROOT` Environment Variable

If `settings.dotlink_root` is not set in your `Link.toml`, Dotlink will use the `DOTLINK_ROOT` environment variable to find your dotfiles directory and the `Link.toml` file within it.

export DOTLINK_ROOT="/home/user/dotfiles"

# Commands

`add`

Moves a file or directory into your dotfiles root, records it in Link.toml, and immediately creates a symlink back to its original location.

#### Usage:

```
dotlink add [TARGETS...]
```

- `TARGETS`: One or more paths to the files or directories you want to start managing. Glob patterns are supported.

#### Example:

```
# Add a single file
dotlink add ~/.gitconfig

# Add a directory
dotlink add ~/.config/alacritty

# Add multiple files using a glob pattern
dotlink add ~/.config/zsh/.z*
```

`unlink`

Removes a symlink, moves the actual file from the dotfiles root back to the symlink's original location, and removes its entry from `Link.toml`.

#### Usage:

```
dotlink unlink [ENTRIES...]
```

- `ENTRIES`: One or more paths to either the symlink or the actual file in the dotfiles root. Glob patterns are supported.

#### Example:

```
# Unlink by specifying the symlink
dotlink unlink ~/.gitconfig

# Unlink by specifying the file in your dotfiles root
dotlink unlink ~/dotfiles/.gitconfig

# Unlink multiple entries
dotlink unlink ~/.config/alacritty ~/.config/nvim
```

`fix`
Scans your `Link.toml` and your filesystem to ensure everything is synchronized. It will:

- Create any missing symlinks.
- Warn about source files that are missing from your dotfiles root.
- Warn about symlinks that point to the wrong place.
- Warn about files that exist at a target location but are not symlinks (conflicts).

#### Usage:

```
dotlink fix
```

This is the primary command for setting up your dotfiles on a new machine or for restoring links after making changes.
