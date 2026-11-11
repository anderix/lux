# Security Policy

## Reporting a vulnerability

Please report suspected vulnerabilities privately, using the **Report a vulnerability** button on the Security tab of this repository. If that button is not there, or you would rather not go through GitHub, email david.anderson@excelano.com instead. I aim to respond within seven days.

Please do not open public issues for security problems.

## Supported versions

The latest release receives security fixes. Older versions are not supported. A fix ships in a new release; update the way you installed.

## What lux can access

lux is a compiler and interpreter that runs locally on your machine. It has no auth layer, stores no credentials, sends no telemetry, and implements no administrative operations. Everything it does runs with the permissions of your operating-system user, and most commands read the `.lux` file you name and write only to the terminal. The commands that reach further are these.

`lux run` and `lux trace` execute the program you gave it, and a lux program has three capabilities that reach outside it. `readFile` and `writeFile` reach any path your operating-system user can reach. `run` starts another program: it takes a program name and a list of arguments rather than a shell string, so there is no shell to inject into and no globbing or redirection, and the child is given empty input, but it does execute, and whatever it executes has your permissions. The language itself has no network access.

**Running a lux program is running code.** It is a general-purpose language, so treat a `.lux` file from someone else as you would a shell script or a Python file from the same source. The language is small and readable, which is a reason to read one before running it rather than a reason not to have to.

A program embedded through the `lux::host` library interface is refused at load time if it uses `readLine`, `input`, `args`, `readFile`, `writeFile` or `run`. Functions a host offers to the program are recorded and returned to the host rather than run, so a hosted program reaches only what its host chooses to act on.

`lux convert` prints the translated Rust, Swift or Go source to standard output and writes no file. For Go it pipes the translation through `gofmt` when `gofmt` is on your PATH, and prints the unformatted source when it is not.

`lux build` translates the program to Rust, writes that source as `<name>.rs` in your system temporary directory, and runs `rustc` from your PATH on it, which writes the binary `./<name>` into the current directory.

`lux crawl` creates a folder in the current directory (`./crawl/`, or the name you give) and writes a starter world and a read-me into it. It refuses to write into a folder that already exists.

`lux editors` runs `--version` on `nvim`, `vim`, `nano` and `gnome-text-editor` to see which are installed, and reads the files listed below to report whether highlighting is in place. `lux editors highlighting` writes lux's highlighting files for each editor it finds, creating directories as needed and replacing a lux file whose contents differ from the copy built into the binary. On Unix those are `~/.config/nvim/{syntax,ftdetect,ftplugin}/lux.vim`, the same three under `~/.vim`, `~/.nano/lux.nanorc` and `~/.local/share/gtksourceview-5/language-specs/lux.lang`; for nano it also appends one `include "~/.nano/lux.nanorc"` line to `~/.nanorc`, leaving the rest of that file as it was. On Windows it writes `%APPDATA%\Notepad++\userDefineLangs\lux.xml`. No other command writes these files.

`lux update` works out how the running lux was installed. It reads the installer receipt described below, checks `CARGO_HOME` (or `~/.cargo/bin`), and on Linux runs `dpkg-query -S` on its own path. For a lux from Homebrew, WinGet, apt or cargo, or one it cannot place, it prints the command that updates that copy and runs nothing. For a lux the shell installer put there, on Unix it runs `sh -c "curl -LsSf https://anderix.com/lux/install | sh"`: it downloads the installer script from anderix.com, which redirects to the latest GitHub release, and executes it, and that script replaces the binary in the installer's user-owned directory. It never uses `sudo`. On Windows it prints the PowerShell installer command to run in a new terminal instead of running it.

## What lux stores

Nothing beyond the files described above. No history file, no cache, no telemetry, no analytics, no remote logging, and lux writes no configuration of its own.

It does read one file it did not write: on Unix, `$XDG_CONFIG_HOME/luxc/luxc-receipt.json` (falling back to `~/.config/luxc`), and on Windows the same under `%LOCALAPPDATA%\luxc`. The installer writes that receipt to record where it installed lux, and `lux update` reads it to know which upgrade route applies. lux writes nothing back to it; the installer that `lux update` runs rewrites it.

## Verifying releases

Every GitHub release includes a `.sha256` file next to each archive, and a `sha256.sum` covering them all. Verify a download before running it:

    sha256sum luxc-x86_64-unknown-linux-gnu.tar.xz
    # compare against the value in luxc-x86_64-unknown-linux-gnu.tar.xz.sha256

Release artifacts are built by `build-release`, the release program in excelano/shipping, from a tagged commit using the cargo-dist configuration in this repository (`dist-workspace.toml`). That build configuration is public and auditable.
