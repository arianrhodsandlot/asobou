use std::path::{Path, PathBuf};

pub fn cores_dir(config_data_dir: Option<&str>) -> PathBuf {
    data_base(config_data_dir).join("cores")
}

pub fn states_dir(config_data_dir: Option<&str>) -> PathBuf {
    data_base(config_data_dir).join("states")
}

pub fn brew_cache_dir(config_cache_dir: Option<&str>) -> PathBuf {
    cache_base(config_cache_dir).join("brew")
}

pub fn data_base(config_data_dir: Option<&str>) -> PathBuf {
    resolve_data_base_from(
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .as_deref(),
        config_data_dir,
        dirs::data_local_dir().as_deref(),
        dirs::home_dir().as_deref(),
    )
}

pub fn cache_base(config_cache_dir: Option<&str>) -> PathBuf {
    resolve_cache_base_from(
        std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .as_deref(),
        config_cache_dir,
        dirs::cache_dir().as_deref(),
        dirs::home_dir().as_deref(),
    )
}

fn resolve_data_base_from(
    xdg_data_home: Option<&Path>,
    config_data_dir: Option<&str>,
    platform: Option<&Path>,
    home: Option<&Path>,
) -> PathBuf {
    if let Some(dir) = config_data_dir {
        return expand_tilde(dir, home);
    }
    if let Some(xdg) = xdg_data_home.filter(|path| !path.as_os_str().is_empty()) {
        return xdg.join("asobou");
    }
    platform
        .expect("could not determine the data directory")
        .join("asobou")
}

fn resolve_cache_base_from(
    xdg_cache_home: Option<&Path>,
    config_cache_dir: Option<&str>,
    platform: Option<&Path>,
    home: Option<&Path>,
) -> PathBuf {
    if let Some(dir) = config_cache_dir {
        return expand_tilde(dir, home);
    }
    if let Some(xdg) = xdg_cache_home.filter(|path| !path.as_os_str().is_empty()) {
        return xdg.join("asobou");
    }
    platform
        .expect("could not determine the cache directory")
        .join("asobou")
}

fn expand_tilde(value: &str, home: Option<&Path>) -> PathBuf {
    if value == "~" {
        return home
            .expect("could not determine the home directory")
            .to_path_buf();
    }
    match value.strip_prefix("~/") {
        Some(rest) => home
            .expect("could not determine the home directory")
            .join(rest),
        None => PathBuf::from(value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_data_dir_wins_over_xdg_env() {
        let base = resolve_data_base_from(
            Some(Path::new("/xdg")),
            Some("/config"),
            Some(Path::new("/platform")),
            Some(Path::new("/home/user")),
        );

        assert_eq!(base, PathBuf::from("/config"));
    }

    #[test]
    fn xdg_data_home_is_used_when_config_is_unset() {
        let base = resolve_data_base_from(
            Some(Path::new("/xdg")),
            None,
            Some(Path::new("/platform")),
            None,
        );

        assert_eq!(base, PathBuf::from("/xdg/asobou"));
    }

    #[test]
    fn empty_xdg_vars_are_treated_as_unset() {
        let base = resolve_data_base_from(
            Some(Path::new("")),
            None,
            Some(Path::new("/platform")),
            None,
        );

        assert_eq!(base, PathBuf::from("/platform/asobou"));
    }

    #[test]
    fn config_data_dir_wins_over_platform() {
        let base =
            resolve_data_base_from(None, Some("/config"), Some(Path::new("/platform")), None);

        assert_eq!(base, PathBuf::from("/config"));
    }

    #[test]
    fn config_data_dir_expands_tilde() {
        let base = resolve_data_base_from(
            None,
            Some("~/games"),
            Some(Path::new("/platform")),
            Some(Path::new("/home/user")),
        );

        assert_eq!(base, PathBuf::from("/home/user/games"));
    }

    #[test]
    fn bare_tilde_expands_to_home() {
        let base = resolve_data_base_from(
            None,
            Some("~"),
            Some(Path::new("/platform")),
            Some(Path::new("/home/user")),
        );

        assert_eq!(base, PathBuf::from("/home/user"));
    }

    #[test]
    fn absolute_config_value_passes_through() {
        let base = resolve_data_base_from(
            None,
            Some("/custom/data"),
            Some(Path::new("/platform")),
            Some(Path::new("/home/user")),
        );

        assert_eq!(base, PathBuf::from("/custom/data"));
    }

    #[test]
    fn platform_fallback_appends_asobou() {
        let base = resolve_data_base_from(None, None, Some(Path::new("/platform")), None);

        assert_eq!(base, PathBuf::from("/platform/asobou"));
    }

    #[test]
    fn cache_resolution_follows_the_same_precedence() {
        let base = resolve_cache_base_from(
            Some(Path::new("/xdg")),
            Some("/config"),
            Some(Path::new("/platform")),
            Some(Path::new("/home/user")),
        );

        assert_eq!(base, PathBuf::from("/config"));

        let base = resolve_cache_base_from(
            Some(Path::new("/xdg")),
            None,
            Some(Path::new("/platform")),
            Some(Path::new("/home/user")),
        );

        assert_eq!(base, PathBuf::from("/xdg/asobou"));

        let base = resolve_cache_base_from(
            None,
            Some("~/cache"),
            Some(Path::new("/platform")),
            Some(Path::new("/home/user")),
        );

        assert_eq!(base, PathBuf::from("/home/user/cache"));
    }

    #[test]
    fn feature_dirs_are_subdirectories_of_the_base() {
        assert_eq!(cores_dir(None), data_base(None).join("cores"));
        assert_eq!(states_dir(None), data_base(None).join("states"));
        assert_eq!(brew_cache_dir(None), cache_base(None).join("brew"));
    }
}
