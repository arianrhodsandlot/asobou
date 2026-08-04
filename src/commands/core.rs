use crate::cores;
use std::path::Path;

pub fn list(cores_dir: &Path) {
    let installed = cores::installed_cores(cores_dir);

    if installed.is_empty() {
        println!("No cores installed.");
        return;
    }

    println!("{:<20} STATUS", "CORE");
    for core in &installed {
        println!("{:<20} installed", core);
    }
}

pub fn install(name: &str, cores_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    if cores::is_installed(name, cores_dir) {
        eprintln!(
            "Core '{}' is already installed. Use 'core update' to refresh it.",
            name
        );
        return Ok(());
    }

    std::fs::create_dir_all(cores_dir)?;
    cores::download_and_install(name, cores_dir, false)?;
    Ok(())
}

pub fn update(name: Option<&str>, cores_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir_all(cores_dir)?;

    let cores_to_update: Vec<String> = if let Some(name) = name {
        if !cores::is_installed(name, cores_dir) {
            eprintln!("Core '{name}' is not installed. Use 'core install' first.");
            std::process::exit(1);
        }
        vec![name.to_string()]
    } else {
        let installed = cores::installed_cores(cores_dir);
        if installed.is_empty() {
            eprintln!("No installed cores to update.");
            return Ok(());
        }
        installed
    };

    for core in &cores_to_update {
        eprintln!("Updating {core} core...");
        cores::download_and_install(core, cores_dir, true)?;
    }

    Ok(())
}

pub fn remove(name: &str, cores_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    if !cores::is_installed(name, cores_dir) {
        eprintln!("Core '{name}' is not installed.");
        return Ok(());
    }

    cores::remove_core_file(name, cores_dir)?;
    eprintln!("Removed {name} core.");
    Ok(())
}
