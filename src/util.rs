#[cfg(target_os = "macos")]
static CONFIG_DIR: &str = "Library/Application Support/rustorcli";

#[cfg(not(target_os = "macos"))]
static CONFIG_DIR: &str = ".rustorcli";

pub fn config_directory() -> String {
    let home = dirs::home_dir().unwrap();
    let home_str = home.to_str().unwrap();
    return format!("{}/{}", home_str, CONFIG_DIR);
}
