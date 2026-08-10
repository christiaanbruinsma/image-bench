pub const BASE_APP_ID: &str = "io.github.christiaanbruinsma.ImageBench";

pub fn app_id() -> &'static str {
    option_env!("APP_ID").unwrap_or(BASE_APP_ID)
}


pub const GETTEXT_PACKAGE: &str = "image-bench";

pub fn locale_dir() -> &'static str {
    option_env!("LOCALEDIR").unwrap_or("/usr/share/locale")
}
