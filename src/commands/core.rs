use crate::cores::{self, CoreEntry};

pub fn list() {
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

pub fn install(name: &str) -> Result<(), Box<dyn std::error::Error>> {
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

    std::fs::create_dir_all(&dir)?;
    cores::download_and_install(core, &dir, false)?;
    Ok(())
}

pub fn update(name: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    let dir = cores::cores_dir();
    std::fs::create_dir_all(&dir)?;

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

    for core in &cores_to_update {
        eprintln!("Updating {} core...", core.name);
        cores::download_and_install(core, &dir, true)?;
    }

    Ok(())
}

pub fn remove(name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let core = cores::find_core(name).ok_or_else(|| {
        format!(
            "Unknown core: '{name}'. Use 'asoby core list' to see available cores."
        )
    })?;

    let dir = cores::cores_dir();

    if !cores::is_installed(core, &dir) {
        eprintln!("Core '{}' is not installed.", core.name);
        return Ok(());
    }

    cores::remove_core_file(core, &dir)?;
    eprintln!("Removed {} core.", core.name);
    Ok(())
}
