//! Open the system default browser to a URL.

use crate::error::{NurError, Result};
use std::process::Command;

fn validate_browser_url(url: &str) -> Result<&str> {
    let url = url.trim();
    if !(url.starts_with("https://") || url.starts_with("http://"))
        || url.chars().any(char::is_control)
    {
        return Err(NurError::Other(
            "refusing to open a non-HTTP OAuth URL".into(),
        ));
    }
    Ok(url)
}

/// Best-effort open of `url` in the platform default browser.
pub fn open_browser(url: &str) -> Result<()> {
    let url = validate_browser_url(url)?;
    #[cfg(target_os = "windows")]
    {
        // Avoid `cmd /C start`: OAuth URLs contain `&`, which cmd interprets
        // as a command separator unless quoting survives two parsers.
        let status = Command::new("rundll32.exe")
            .args(["url.dll,FileProtocolHandler", url])
            .status()
            .map_err(|e| NurError::Other(format!("failed to open browser: {e}")))?;
        if !status.success() {
            return Err(NurError::Other(
                "browser open command failed — open the URL manually".into(),
            ));
        }
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        let status = Command::new("open")
            .arg(url)
            .status()
            .map_err(|e| NurError::Other(format!("failed to open browser: {e}")))?;
        if !status.success() {
            return Err(NurError::Other(
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
        Err(NurError::Other(
            "could not open a browser — open the URL manually".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_opener_accepts_only_http_urls_without_controls() {
        assert!(validate_browser_url("https://example.com/oauth?a=1&b=2").is_ok());
        assert!(validate_browser_url("http://localhost:1455/callback").is_ok());
        assert!(validate_browser_url("file:///tmp/token").is_err());
        assert!(validate_browser_url("https://example.com\r\nmalicious").is_err());
    }
}
