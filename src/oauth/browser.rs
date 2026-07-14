//! Open the system default browser to a URL.

use crate::error::{MuseError, Result};
use std::process::Command;

/// Best-effort open of `url` in the platform default browser.
pub fn open_browser(url: &str) -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        // `start` needs an empty title arg when the URL is quoted.
        let status = Command::new("cmd")
            .args(["/C", "start", "", url])
            .status()
            .map_err(|e| MuseError::Other(format!("failed to open browser: {e}")))?;
        if !status.success() {
            return Err(MuseError::Other(
                "browser open command failed — open the URL manually".into(),
            ));
        }
        return Ok(());
    }
    #[cfg(target_os = "macos")]
    {
        let status = Command::new("open")
            .arg(url)
            .status()
            .map_err(|e| MuseError::Other(format!("failed to open browser: {e}")))?;
        if !status.success() {
            return Err(MuseError::Other(
                "browser open command failed — open the URL manually".into(),
            ));
        }
        return Ok(());
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        for bin in ["xdg-open", "gio", "gnome-open"] {
            if let Ok(status) = Command::new(bin).arg(url).status() {
                if status.success() {
                    return Ok(());
                }
            }
        }
        Err(MuseError::Other(
            "could not open a browser — open the URL manually".into(),
        ))
    }
}
