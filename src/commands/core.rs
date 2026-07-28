use std::io::IsTerminal;

use crate::cores::{self, CoreEntry};

fn prompt_yes(question: &str, default_yes: bool) -> bool {
    if default_yes {
        eprint!("{question} [Y/n] ");
    } else {
        eprint!("{question} [y/N] ");
    }
    use std::io::{BufRead, Write};
    let _ = std::io::stderr().flush();
    let stdin = std::io::stdin();
    let mut line = String::new();
    if stdin.lock().read_line(&mut line).is_err() {
        return default_yes;
    }
    let answer = line.trim().to_ascii_lowercase();
    if answer.is_empty() {
        return default_yes;
    }
    answer.starts_with('y')
}

fn confirm_remove(core: &CoreEntry, interactive: bool, yes: bool) -> bool {
    if yes {
        return true;
    }
    if !interactive {
        eprintln!("Removing a core requires --yes when not in an interactive terminal.");
        return false;
    }
    prompt_yes(
        &format!("Remove {} core?", core.name),
        false,
    )
}

pub fn list(yes: bool, no_download: bool) {
    let _ = (yes, no_download);

    let dir = cores::cores_dir();
    let installed = cores::installed_cores(&dir);

    if installed.is_empty() {
        println!("No cores installed.");
        return;
    }

    println!("{:<20} STATUS", "CORE");
    for core in &installed {
        println!("{:<20} installed", core.name);
    }
}

pub fn install(name: &str, yes: bool, no_download: bool) -> Result<(), Box<dyn std::error::Error>> {
    let _ = yes;
    let core = cores::find_core(name).ok_or_else(|| {
        format!(
            "Unknown core: '{name}'. Use 'asoby core list' to see available cores."
        )
    })?;

    let dir = cores::cores_dir();

    if cores::is_installed(core, &dir) {
        eprintln!(
            "Core '{}' is already installed. Use 'core update' to refresh it.",
            core.name
        );
        return Ok(());
    }

    if no_download {
        eprintln!("--no-download prevents installing cores.");
        std::process::exit(1);
    }

    std::fs::create_dir_all(&dir)?;
    cores::download_and_install(core, &dir, false)?;
    Ok(())
}

pub fn update(name: Option<&str>, yes: bool, no_download: bool) -> Result<(), Box<dyn std::error::Error>> {
    let interactive = std::io::stdin().is_terminal();
    let dir = cores::cores_dir();
    std::fs::create_dir_all(&dir)?;

    if no_download {
        eprintln!("--no-download prevents updating cores.");
        std::process::exit(1);
    }

    let cores_to_update: Vec<&CoreEntry> = if let Some(name) = name {
        let core = cores::find_core(name).ok_or_else(|| {
            format!(
                "Unknown core: '{name}'. Use 'asoby core list' to see available cores."
            )
        })?;
        if !cores::is_installed(core, &dir) {
            eprintln!("Core '{}' is not installed. Use 'core install' first.", core.name);
            std::process::exit(1);
        }
        vec![core]
    } else {
        let installed = cores::installed_cores(&dir);
        if installed.is_empty() {
            eprintln!("No installed cores to update.");
            return Ok(());
        }
        installed
    };

    if !yes && !interactive {
        eprintln!("Updating cores requires --yes when not in an interactive terminal.");
        std::process::exit(1);
    }

    for core in &cores_to_update {
        if !yes && interactive {
            if !prompt_yes(
                &format!(
                    "Update {} core from buildbot.libretro.com to {}?",
                    core.name,
                    dir.display()
                ),
                true,
            ) {
                continue;
            }
        }
        eprintln!("Updating {} core...", core.name);
        cores::download_and_install(core, &dir, true)?;
    }

    Ok(())
}

pub fn remove(name: &str, yes: bool) -> Result<(), Box<dyn std::error::Error>> {
    let core = cores::find_core(name).ok_or_else(|| {
        format!(
            "Unknown core: '{name}'. Use 'asoby core list' to see available cores."
        )
    })?;

    let interactive = std::io::stdin().is_terminal();
    let dir = cores::cores_dir();

    if !cores::is_installed(core, &dir) {
        eprintln!("Core '{}' is not installed.", core.name);
        return Ok(());
    }

    if !confirm_remove(core, interactive, yes) {
        std::process::exit(1);
    }

    cores::remove_core_file(core, &dir)?;
    eprintln!("Removed {} core.", core.name);
    Ok(())
}
