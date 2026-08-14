# Security Policy

## Reporting a vulnerability

Please report suspected vulnerabilities privately through GitHub Security Advisories at https://github.com/anderix/lux/security/advisories/new. If you would rather not use GitHub, email david.anderson@excelano.com instead. I aim to respond within seven days.

Please do not open public issues for security problems.

## Supported versions

The latest 0.x release receives security fixes. Older versions are not supported.

## What lux can access

lux is a compiler and interpreter that runs locally on your machine. It reads the `.lux` source files you point it at, writes the output you ask for, and exits. It makes no network calls, has no auth layer, stores no credentials, and implements no administrative operations. It can only read and write files your operating-system user already has access to.

`lux run` executes the program you gave it, and a lux program has three capabilities worth knowing about. `readFile` and `writeFile` reach any path your operating-system user can reach. `run` starts another program: it takes a program name and a list of arguments rather than a shell string, so there is no shell to inject into and no globbing or redirection, and the child is given empty input — but it does execute, and whatever it executes has your permissions. The language has no network access of any kind.

**Running a lux program is running code.** It is a general-purpose language, so treat a `.lux` file from someone else exactly as you would a shell script or a Python file from the same source. The language is small and readable, which is the point of it, and that is a reason to read one before running it rather than a reason not to have to.

`lux convert` writes Rust, Swift, or Go source next to your program. Compiling or running that output is a separate step you take with a separate toolchain; lux does not invoke one.

`lux update` checks the GitHub releases API for a newer version and, depending on how lux was installed, either downloads that release or prints the command your package manager wants. It installs nothing you did not ask it to.

## What lux stores

Nothing beyond the files you asked for. No history file, no cache, no telemetry, no analytics, no remote logging, and lux writes no configuration of its own.

It does *read* one file it did not write: on Unix, `$XDG_CONFIG_HOME/luxc/luxc-receipt.json` (falling back to `~/.config/luxc`), and on Windows the same under `%LOCALAPPDATA%\luxc`. The cargo-dist installer writes that receipt to record where it installed lux, and `lux update` reads it to know which upgrade route applies. Nothing is written back to it.

## Verifying releases

Every GitHub release includes a `.sha256` file next to each archive listing its SHA-256 hash. Verify any download before running it:

    sha256sum luxc-x86_64-unknown-linux-gnu.tar.xz
    # compare against the value in luxc-x86_64-unknown-linux-gnu.tar.xz.sha256

Release artifacts are built by GitHub Actions from a tagged commit using the cargo-dist configuration in this repo (`dist-workspace.toml` and the generated `.github/workflows/release.yml`). The workflow and build configuration are public and auditable.
