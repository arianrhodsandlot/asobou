use std::collections::HashMap;
use std::sync::LazyLock;

type Map = HashMap<&'static str, &'static str>;

static MAP: LazyLock<Map> = LazyLock::new(|| {
    HashMap::from([
        ("nes", "fceumm"),
        ("fds", "fceumm"),
        ("unf", "fceumm"),
        ("unif", "fceumm"),
        ("sfc", "snes9x"),
        ("smc", "snes9x"),
        ("md", "genesis_plus_gx"),
        ("bin", "genesis_plus_gx"),
        ("gen", "genesis_plus_gx"),
        ("sms", "genesis_plus_gx"),
        ("gg", "genesis_plus_gx"),
        ("sg", "genesis_plus_gx"),
        ("gb", "mgba"),
        ("gbc", "mgba"),
        ("gba", "mgba"),
        ("a52", "a5200"),
    ])
});

pub fn for_rom(path: &std::path::Path) -> &'static str {
    path.extension()
        .and_then(|e| e.to_str())
        .and_then(|e| MAP.get(e))
        .copied()
        .unwrap_or("nestopia")
}
