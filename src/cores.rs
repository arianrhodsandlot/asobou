use std::collections::HashMap;
use std::sync::LazyLock;

type Map = HashMap<&'static str, &'static str>;

static MAP: LazyLock<Map> = LazyLock::new(|| {
    HashMap::from([
        ("32x", "picodrive"),
        ("a26", "stella"),
        ("a52", "a5200"),
        ("bin", "genesis_plus_gx"),
        ("fds", "fceumm"),
        ("gb", "mgba"),
        ("gba", "mgba"),
        ("gbc", "mgba"),
        ("gen", "genesis_plus_gx"),
        ("gg", "genesis_plus_gx"),
        ("md", "genesis_plus_gx"),
        ("nes", "fceumm"),
        ("sfc", "snes9x"),
        ("sg", "genesis_plus_gx"),
        ("smc", "snes9x"),
        ("sms", "genesis_plus_gx"),
        ("unf", "fceumm"),
        ("unif", "fceumm"),
    ])
});

fn extension(path: &std::path::Path) -> Option<String> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    if ext == "zip" {
        let file = std::fs::File::open(path).ok()?;
        let mut archive = zip::ZipArchive::new(file).ok()?;
        let first = archive.by_index(0).ok()?;
        let name = first.name().to_string();
        return std::path::Path::new(&name)
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase());
    }
    Some(ext)
}

pub fn for_rom(path: &std::path::Path) -> &'static str {
    extension(path)
        .as_deref()
        .and_then(|e| MAP.get(e))
        .copied()
        .unwrap_or("nestopia")
}
