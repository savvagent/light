//! Open the user's default browser, used by the device-login flow.

use std::process::Command;

/// Open `url` in the default browser, best-effort cross-platform.
pub fn open_browser(url: &str) -> anyhow::Result<()> {
    #[cfg(target_os = "macos")]
    let (program, arg) = ("open", url.to_string());

    #[cfg(target_os = "windows")]
    let (program, arg) = ("cmd", format!("/C start \"\" {url}"));

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let (program, arg) = ("xdg-open", url.to_string());

    Command::new(program).arg(arg).spawn()?;
    Ok(())
}
