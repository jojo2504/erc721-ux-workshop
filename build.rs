//! Exposes `.env` entries to the crate as compile-time env vars (`option_env!`).
//! Shell env vars win over `.env`.

fn main() {
    println!("cargo:rerun-if-changed=.env");
    let Ok(contents) = std::fs::read_to_string(".env") else { return };
    for line in contents.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim();
            println!("cargo:rerun-if-env-changed={key}");
            if std::env::var_os(key).is_none() {
                println!("cargo:rustc-env={key}={}", value.trim().trim_matches('"'));
            }
        }
    }
}
