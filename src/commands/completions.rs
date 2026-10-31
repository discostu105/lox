//! `lox completions`: print or install shell completions.

use anyhow::{Result, bail};
use clap::CommandFactory;
use clap_complete::{Shell, generate};
use dirs::home_dir;
use std::fs;

use crate::Cli;
use crate::commands::RunContext;

pub fn cmd_completions(_ctx: &RunContext, shell: Option<Shell>, install: bool) -> Result<()> {
    let detected = shell.or_else(detect_shell);
    let Some(sh) = detected else {
        bail!("Could not detect shell. Specify one: lox completions bash|zsh|fish");
    };
    let mut cmd = Cli::command();
    if install {
        install_completions(sh, &mut cmd)?;
    } else {
        // Through `print!`, so `lox completions bash | head` exits quietly.
        let mut buf = Vec::new();
        generate(sh, &mut cmd, "lox", &mut buf);
        print!("{}", String::from_utf8_lossy(&buf));
    }
    Ok(())
}

fn detect_shell() -> Option<Shell> {
    // On Unix, $SHELL is the login shell
    if let Some(shell) = std::env::var("SHELL").ok().and_then(|s| {
        let name = s.rsplit('/').next().unwrap_or(&s);
        match name {
            "bash" => Some(Shell::Bash),
            "zsh" => Some(Shell::Zsh),
            "fish" => Some(Shell::Fish),
            "elvish" => Some(Shell::Elvish),
            "pwsh" | "powershell" => Some(Shell::PowerShell),
            _ => None,
        }
    }) {
        return Some(shell);
    }
    // On Windows, $SHELL is not set; check for PowerShell via PSModulePath
    if std::env::var("PSModulePath").is_ok() {
        return Some(Shell::PowerShell);
    }
    None
}

fn install_completions(shell: Shell, cmd: &mut clap::Command) -> Result<()> {
    // Check LOX_HOME first (for testing), then HOME, then dirs::home_dir()
    let home = std::env::var("LOX_HOME")
        .ok()
        .map(std::path::PathBuf::from)
        .or_else(home_dir)
        .unwrap_or_default();
    let (path, label) = match shell {
        Shell::Bash => {
            let dir = home.join(".local/share/bash-completion/completions");
            fs::create_dir_all(&dir)?;
            (dir.join("lox"), "bash")
        }
        Shell::Zsh => {
            let dir = home.join(".zfunc");
            fs::create_dir_all(&dir)?;
            (dir.join("_lox"), "zsh")
        }
        Shell::Fish => {
            let dir = home.join(".config/fish/completions");
            fs::create_dir_all(&dir)?;
            (dir.join("lox.fish"), "fish")
        }
        Shell::PowerShell => {
            // Install as a PowerShell profile script
            let documents = if cfg!(windows) {
                home.join("Documents")
            } else {
                home.join(".config")
            };
            let dir = documents.join("PowerShell");
            fs::create_dir_all(&dir)?;
            (dir.join("lox_completions.ps1"), "powershell")
        }
        Shell::Elvish => {
            bail!("Elvish completions must be installed manually — run: lox completions elvish");
        }
        _ => bail!("Unsupported shell"),
    };
    let mut buf = Vec::new();
    generate(shell, cmd, "lox", &mut buf);
    fs::write(&path, &buf)?;

    eprintln!("Installed {} completions to {}", label, path.display());
    if shell == Shell::Zsh {
        eprintln!("Ensure ~/.zfunc is in your fpath. Add to ~/.zshrc:");
        eprintln!("  fpath=(~/.zfunc $fpath)");
        eprintln!("  autoload -Uz compinit && compinit");
    }
    if shell == Shell::PowerShell {
        eprintln!("Add this line to your PowerShell $PROFILE to load completions:");
        eprintln!("  . \"{}\"", path.display());
    }
    eprintln!("Restart your shell or open a new tab to activate.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_completions_bash_generates_output() {
        let mut cmd = Cli::command();
        let mut buf = Vec::new();
        generate(Shell::Bash, &mut cmd, "lox", &mut buf);
        let output = String::from_utf8(buf).unwrap();
        assert!(
            output.contains("_lox"),
            "bash completions should define _lox function"
        );
        assert!(
            output.contains("COMPREPLY"),
            "bash completions should use COMPREPLY"
        );
    }

    #[test]
    fn test_completions_zsh_generates_output() {
        let mut cmd = Cli::command();
        let mut buf = Vec::new();
        generate(Shell::Zsh, &mut cmd, "lox", &mut buf);
        let output = String::from_utf8(buf).unwrap();
        assert!(
            output.contains("compdef"),
            "zsh completions should contain compdef"
        );
    }

    #[test]
    fn test_completions_fish_generates_output() {
        let mut cmd = Cli::command();
        let mut buf = Vec::new();
        generate(Shell::Fish, &mut cmd, "lox", &mut buf);
        let output = String::from_utf8(buf).unwrap();
        assert!(
            output.contains("complete -c lox"),
            "fish completions should define completions for lox"
        );
    }

    #[test]
    fn test_detect_shell_from_env() {
        // detect_shell reads $SHELL — just verify it returns Some for known shells
        // or None for unknown ones (depends on test environment)
        let result = detect_shell();
        // In CI $SHELL may or may not be set; just ensure no panic
        let _ = result;
    }

    #[test]
    fn test_install_completions_creates_file() {
        let tmp = std::env::temp_dir().join("lox_test_completions");
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        // Use LOX_HOME to override home directory for the test (works on all platforms)
        // SAFETY: Test runs serially; no other threads access this env var here.
        unsafe { std::env::set_var("LOX_HOME", tmp.to_str().unwrap()) };

        let mut cmd = Cli::command();

        if cfg!(windows) {
            let result = install_completions(Shell::PowerShell, &mut cmd);
            assert!(
                result.is_ok(),
                "install_completions should succeed for powershell"
            );
            let ps_file = tmp.join("Documents/PowerShell/lox_completions.ps1");
            assert!(
                ps_file.exists(),
                "powershell completion file should be created"
            );
        } else {
            let result = install_completions(Shell::Bash, &mut cmd);
            assert!(
                result.is_ok(),
                "install_completions should succeed for bash"
            );
            let bash_file = tmp.join(".local/share/bash-completion/completions/lox");
            assert!(bash_file.exists(), "bash completion file should be created");
            let content = fs::read_to_string(&bash_file).unwrap();
            assert!(
                content.contains("_lox"),
                "installed file should contain bash completions"
            );
        }

        unsafe { std::env::remove_var("LOX_HOME") };
        let _ = fs::remove_dir_all(&tmp);
    }
}
