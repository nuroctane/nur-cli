use crate::config::{auth_path, ensure_dirs};
use crate::error::{MuseError, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Write};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Auth {
    pub api_key: String,
    #[serde(default)]
    pub source: String,
}

/// Resolve API key: META_API_KEY → MUSE_API_KEY → MODEL_API_KEY → ~/.muse/auth.json
pub fn resolve_api_key() -> Result<String> {
    for var in ["META_API_KEY", "MUSE_API_KEY", "MODEL_API_KEY"] {
        if let Ok(k) = std::env::var(var) {
            let k = k.trim().to_string();
            if !k.is_empty() {
                return Ok(k);
            }
        }
    }
    let path = auth_path();
    if path.exists() {
        let text = fs::read_to_string(&path)?;
        let auth: Auth = serde_json::from_str(&text)?;
        let k = auth.api_key.trim().to_string();
        if !k.is_empty() {
            return Ok(k);
        }
    }
    Err(MuseError::NotAuthenticated)
}

pub fn save_api_key(key: &str) -> Result<()> {
    ensure_dirs()?;
    let trimmed = key.trim();
    if trimmed.len() < 20 {
        return Err(MuseError::Other(
            "API key too short — expected Meta API key (min 20 chars)".into(),
        ));
    }
    if trimmed.contains(' ') || trimmed.contains('\n') {
        return Err(MuseError::Other("API key contains whitespace".into()));
    }
    let auth = Auth {
        api_key: trimmed.to_string(),
        source: "login".to_string(),
    };
    let text = serde_json::to_string_pretty(&auth)?;
    let path = auth_path();
    crate::config::atomic_write(&path, text.as_bytes())
        .map_err(|e| MuseError::Other(format!("failed to save auth atomically: {e}")))?;
    // Restrictive perms on Unix. On Windows, ~/.muse under the user profile is
    // already private via default NTFS ACLs — no portable 0600 equivalent.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

pub fn logout() -> Result<()> {
    let path = auth_path();
    if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

pub fn key_fingerprint(key: &str) -> String {
    let k = key.trim();
    if k.len() <= 8 {
        return "****".to_string();
    }
    format!("{}…{}", &k[..4], &k[k.len() - 4..])
}

pub fn auth_status() -> Result<()> {
    match resolve_api_key() {
        Ok(key) => {
            let source = if std::env::var("META_API_KEY").is_ok() {
                "META_API_KEY env"
            } else if std::env::var("MUSE_API_KEY").is_ok() {
                "MUSE_API_KEY env"
            } else if std::env::var("MODEL_API_KEY").is_ok() {
                "MODEL_API_KEY env"
            } else {
                "~/.muse/auth.json"
            };
            println!("authenticated: yes");
            println!("source: {source}");
            println!("key: {}", key_fingerprint(&key));
            Ok(())
        }
        Err(MuseError::NotAuthenticated) => {
            println!("authenticated: no");
            println!("run: muse auth login");
            println!("or set MODEL_API_KEY / MUSE_API_KEY");
            Ok(())
        }
        Err(e) => Err(e),
    }
}

pub fn login_interactive(key_arg: Option<String>) -> Result<()> {
    let key = if let Some(k) = key_arg {
        k
    } else {
        print!("Meta Model API key: ");
        io::stdout().flush()?;
        // Prefer silent input when possible
        match rpassword::read_password() {
            Ok(k) if !k.trim().is_empty() => k,
            _ => {
                // Fallback visible line
                let mut line = String::new();
                io::stdin().read_line(&mut line)?;
                line
            }
        }
    };
    let key = key.trim();
    if key.is_empty() {
        return Err(MuseError::Other("empty API key".into()));
    }
    save_api_key(key)?;
    println!("saved to {}", auth_path().display());
    println!("key: {}", key_fingerprint(key));
    Ok(())
}
