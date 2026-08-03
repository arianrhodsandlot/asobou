pub fn edit() -> Result<(), Box<dyn std::error::Error>> {
    crate::config::edit()?;
    Ok(())
}

pub fn list() -> Result<(), Box<dyn std::error::Error>> {
    let entries = crate::config::list()?;
    let key_width = entries
        .iter()
        .map(|entry| entry.key.len())
        .max()
        .unwrap_or(3)
        .max(3);
    let value_width = entries
        .iter()
        .map(|entry| entry.value.len())
        .max()
        .unwrap_or(5)
        .max(5);
    println!("{:<key_width$}  {:<value_width$}  SOURCE", "KEY", "VALUE");
    for entry in entries {
        let source = if entry.configured {
            "config"
        } else {
            "default"
        };
        println!(
            "{:<key_width$}  {:<value_width$}  {source}",
            entry.key, entry.value
        );
    }
    Ok(())
}

pub fn get(key: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("{}", crate::config::get(key)?);
    Ok(())
}

pub fn set(key: &str, value: &str) -> Result<(), Box<dyn std::error::Error>> {
    let value = crate::config::set(key, value)?;
    println!("{key} = {value}");
    Ok(())
}

pub fn unset(key: &str) -> Result<(), Box<dyn std::error::Error>> {
    let value = crate::config::unset(key)?;
    println!("{key} = {}", value.as_deref().unwrap_or("<unset>"));
    Ok(())
}
