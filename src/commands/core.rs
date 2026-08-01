use crate::cores;

pub fn list() {
    let dir = cores::cores_dir();
    let installed = cores::installed_cores(&dir);

    if installed.is_empty() {
        println!("No cores installed.");
        return;
    }

    println!("{:<20} STATUS", "CORE");
    for core in &installed {
        println!("{:<20} installed", core);
    }
}

pub fn install(name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let dir = cores::cores_dir();

    if cores::is_installed(name, &dir) {
        eprintln!(
            "Core '{}' is already installed. Use 'core update' to refresh it.",
            name
        );
        return Ok(());
    }

    std::fs::create_dir_all(&dir)?;
    cores::download_and_install(name, &dir, false)?;
    Ok(())
}

pub fn update(name: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    let dir = cores::cores_dir();
    std::fs::create_dir_all(&dir)?;

    let cores_to_update: Vec<String> = if let Some(name) = name {
        if !cores::is_installed(name, &dir) {
            eprintln!("Core '{name}' is not installed. Use 'core install' first.");
            std::process::exit(1);
        }
        vec![name.to_string()]
    } else {
        let installed = cores::installed_cores(&dir);
        if installed.is_empty() {
            eprintln!("No installed cores to update.");
            return Ok(());
        }
        installed
    };

    for core in &cores_to_update {
        eprintln!("Updating {core} core...");
        cores::download_and_install(core, &dir, true)?;
    }

    Ok(())
}

pub fn remove(name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let dir = cores::cores_dir();

    if !cores::is_installed(name, &dir) {
        eprintln!("Core '{name}' is not installed.");
        return Ok(());
    }

    cores::remove_core_file(name, &dir)?;
    eprintln!("Removed {name} core.");
    Ok(())
}
