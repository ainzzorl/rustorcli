// TODO: what if not on mac?
static CONFIG_DIR: &str = "Library/Application Support/rustorcli";

pub fn config_directory() -> String {
    let home = dirs::home_dir().unwrap();
    let home_str = home.to_str().unwrap();
    return format!("{}/{}", home_str, CONFIG_DIR);
}
