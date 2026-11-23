//! Which copy of the app is running: the installed one, or a development build.
//!
//! `scripts/dev.sh` starts the app with `MEAT_PROXY_PROFILE=dev`. That copy has its own name, its
//! own saved data, settings and caches, and its own macOS bundle id, so it never touches the
//! installed app's pull requests, pins or notification permission.

pub fn is_dev() -> bool {
    std::env::var("MEAT_PROXY_PROFILE").is_ok_and(|v| v.trim() == "dev")
}

/// The name shown to the user.
pub fn app_name() -> &'static str {
    if is_dev() { "Meat Proxy Dev" } else { "Meat Proxy" }
}

/// The folder name under `~/.config` and the caches directory.
pub fn dir_name() -> &'static str {
    if is_dev() { "meat-proxy-dev" } else { "meat-proxy" }
}
