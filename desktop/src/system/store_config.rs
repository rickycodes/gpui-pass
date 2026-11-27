use std::{
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoreConfig {
    store_path: PathBuf,
}

pub(crate) fn resolve_store_path(cli: Option<PathBuf>) -> Result<Option<PathBuf>, String> {
    let env_path = env::var_os("PASSWORD_STORE_DIR")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from);
    let saved_path = if cli.is_none() && env_path.is_none() {
        load_store_path()
            .map_err(|error| format!("Could not read gpui-pass configuration: {error}"))?
    } else {
        None
    };
    let default_path = if cli.is_none() && env_path.is_none() && saved_path.is_none() {
        match gpui_pass_core::default_store_path() {
            Ok(path) => Some(path),
            Err(gpui_pass_core::Error::MissingHome) => None,
            Err(error) => return Err(error.to_string()),
        }
    } else {
        None
    };
    Ok(choose_store_path(cli, env_path, saved_path, default_path))
}

fn choose_store_path(
    cli: Option<PathBuf>,
    env_path: Option<PathBuf>,
    saved_path: Option<PathBuf>,
    default_path: Option<PathBuf>,
) -> Option<PathBuf> {
    cli.or(env_path).or(saved_path).or(default_path)
}

pub(crate) fn save_store_path(path: &Path) -> io::Result<()> {
    let path = if path.is_absolute() {
        path.to_owned()
    } else {
        env::current_dir()?.join(path)
    };
    let config_path = config_path()?.ok_or_else(|| {
        io::Error::new(io::ErrorKind::NotFound, "no home or XDG config directory")
    })?;
    let parent = config_path.parent().expect("config path has a parent");
    fs::create_dir_all(parent)?;

    let contents = toml::to_string(&StoreConfig { store_path: path })
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let temp_path = parent.join(format!(
        ".store-config.{}.{}.tmp",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let result = write_config_atomically(&temp_path, &config_path, contents.as_bytes());
    if result.is_err() {
        let _ = fs::remove_file(temp_path);
    }
    result
}

fn load_store_path() -> io::Result<Option<PathBuf>> {
    let Some(path) = config_path()? else {
        return Ok(None);
    };
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    parse_store_path(&contents)
}

fn parse_store_path(contents: &str) -> io::Result<Option<PathBuf>> {
    let config: StoreConfig = toml::from_str(contents)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if !config.store_path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "store_path must be absolute",
        ));
    }
    Ok(Some(config.store_path))
}

fn config_path() -> io::Result<Option<PathBuf>> {
    let xdg = env::var_os("XDG_CONFIG_HOME").filter(|path| !path.is_empty());
    let home = env::var_os("HOME").filter(|home| !home.is_empty());
    config_path_from(xdg, home)
}

fn config_path_from(
    xdg_config_home: Option<std::ffi::OsString>,
    home: Option<std::ffi::OsString>,
) -> io::Result<Option<PathBuf>> {
    if let Some(path) = xdg_config_home {
        let path = PathBuf::from(path);
        if !path.is_absolute() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "XDG_CONFIG_HOME must be absolute",
            ));
        }
        return Ok(Some(path.join("gpui-pass/config.toml")));
    }
    Ok(home.map(|home| PathBuf::from(home).join(".config/gpui-pass/config.toml")))
}

fn write_config_atomically(
    temp_path: &Path,
    config_path: &Path,
    contents: &[u8],
) -> io::Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(temp_path)?;
    file.write_all(contents)?;
    file.sync_all()?;
    drop(file);
    fs::rename(temp_path, config_path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn temp_dir(label: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock is after the Unix epoch")
            .as_nanos();
        let path = env::temp_dir().join(format!(
            "gpui-pass-config-{label}-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("create temporary directory");
        path
    }

    #[test]
    fn config_round_trip_preserves_spaces_and_requires_absolute_paths() {
        let root = temp_dir("round-trip");
        let store = root.join("vault with spaces");
        fs::create_dir(&store).expect("create store");
        let config = root.join("config.toml");
        let temp = root.join("config.tmp");
        let serialized = toml::to_string(&StoreConfig {
            store_path: store.clone(),
        })
        .expect("serialize config");
        write_config_atomically(&temp, &config, serialized.as_bytes()).expect("write config");
        assert_eq!(
            parse_store_path(&fs::read_to_string(&config).unwrap()).unwrap(),
            Some(store)
        );
        assert!(parse_store_path("store_path = \"relative/path\"\n").is_err());
        assert!(parse_store_path("not toml").is_err());
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn cli_and_environment_override_saved_config() {
        let cli = PathBuf::from("/cli");
        let environment = PathBuf::from("/environment");
        let saved = PathBuf::from("/saved");
        let fallback = PathBuf::from("/default");

        assert_eq!(
            choose_store_path(
                Some(cli.clone()),
                Some(environment.clone()),
                Some(saved.clone()),
                Some(fallback.clone())
            ),
            Some(cli)
        );
        assert_eq!(
            choose_store_path(
                None,
                Some(environment.clone()),
                Some(saved.clone()),
                Some(fallback.clone())
            ),
            Some(environment)
        );
        assert_eq!(
            choose_store_path(None, None, Some(saved.clone()), Some(fallback)),
            Some(saved)
        );
    }

    #[test]
    fn config_path_follows_xdg_or_home_defaults() {
        let xdg = PathBuf::from("/custom/config");
        assert_eq!(
            config_path_from(Some(xdg.clone().into()), Some("/home/user".into())).unwrap(),
            Some(xdg.join("gpui-pass/config.toml"))
        );
        assert_eq!(
            config_path_from(None, Some("/home/user".into())).unwrap(),
            Some(PathBuf::from("/home/user/.config/gpui-pass/config.toml"))
        );
        assert!(config_path_from(Some("relative".into()), None).is_err());
    }
}
