//! At-rest encryption for the credential files (`auth.json`,
//! `provider_keys.json`, `provider_sessions.json`, `credential_policy.json`).
//!
//! File permissions are the first line and stay: Unix 0600, Windows an
//! explicit owner-only DACL on the temp file before the rename
//! ([`crate::config::private_atomic_write`]). They do not survive a copy,
//! though - a backup to a share, a sync client, a `tar` of the profile hands
//! the tokens to whoever reads it next.
//!
//! On Windows the bytes are therefore sealed with DPAPI (`CryptProtectData`,
//! user scope) before they are written: the key belongs to the Windows
//! account and the machine, so a copied file is inert elsewhere. Unix has no
//! equivalent that works without a desktop keyring daemon, so there the mode
//! is the protection and the file stays plain JSON.
//!
//! The envelope is a single text line, `nur-dpapi:v1:<base64 blob>`, so a
//! reader can tell a sealed file from JSON without parsing it. Reads accept
//! both; a plaintext file is re-sealed in place the first time it is read,
//! which migrates an existing install without a re-login.

use base64::Engine;
use std::io;
use std::path::Path;

/// Marks a sealed file. Chosen to be invalid JSON so a stray plaintext
/// reader fails loudly instead of treating ciphertext as a credential.
const ENVELOPE_PREFIX: &str = "nur-dpapi:v1:";

/// Entropy mixed into the DPAPI blob: another process on the same account
/// cannot unseal a stolen file without knowing it. Not a secret, and not a
/// password - it only scopes the key to this application.
#[cfg(windows)]
const ENTROPY: &[u8] = b"nur-cli credential store v1";

/// Read a credential file, unsealing it when it is sealed.
///
/// `ErrorKind::InvalidData` means the file is a sealed envelope this account
/// cannot open (copied from another user or machine, or truncated). Callers
/// treat that as "no usable credential", never as plaintext.
pub fn read_to_string(path: &Path) -> io::Result<String> {
    read_with(path, sealing_available())
}

/// Write a credential file, sealing it when this platform and config allow.
/// Sealing that fails is never fatal: the credential is written as before so
/// a login cannot be lost to a DPAPI error.
pub fn write(path: &Path, text: &str) -> io::Result<()> {
    write_with(path, text, sealing_available())
}

/// [`read_to_string`] with the sealing decision supplied, so the envelope can
/// be exercised without a config or a home directory.
fn read_with(path: &Path, seal: bool) -> io::Result<String> {
    let raw = std::fs::read_to_string(path)?;
    let Some(encoded) = raw.trim().strip_prefix(ENVELOPE_PREFIX) else {
        // Plaintext from an older build (or a platform without sealing).
        migrate_in_place(path, &raw, seal);
        return Ok(raw);
    };
    let blob = base64::engine::general_purpose::STANDARD
        .decode(encoded.trim())
        .map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "{} is not a readable credential envelope: {e}",
                    path.display()
                ),
            )
        })?;
    let plain = unprotect(&blob).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "{} was encrypted for a different Windows account or machine - sign in again, \
                 or set [auth] encrypt_at_rest = false if you sync ~/.nur",
                path.display()
            ),
        )
    })?;
    String::from_utf8(plain).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{} decrypted to invalid UTF-8: {e}", path.display()),
        )
    })
}

fn write_with(path: &Path, text: &str, seal: bool) -> io::Result<()> {
    match sealed_bytes(text, seal) {
        Some(envelope) => crate::config::private_atomic_write(path, envelope.as_bytes()),
        None => crate::config::private_atomic_write(path, text.as_bytes()),
    }
}

/// Whether what is on disk at `path` is sealed. For diagnostics (`/auth`,
/// `nur doctor`); the read path does not need to ask.
pub fn is_sealed(path: &Path) -> bool {
    let mut buf = [0u8; 16];
    use std::io::Read;
    let Ok(mut f) = std::fs::File::open(path) else {
        return false;
    };
    let Ok(n) = f.read(&mut buf) else {
        return false;
    };
    String::from_utf8_lossy(&buf[..n]).starts_with(ENVELOPE_PREFIX)
}

/// True when new credential writes on this machine will be sealed.
pub fn sealing_available() -> bool {
    cfg!(windows) && encrypt_at_rest_enabled()
}

fn encrypt_at_rest_enabled() -> bool {
    crate::config::load_config()
        .map(|c| c.auth.encrypt_at_rest)
        .unwrap_or(true)
}

/// The envelope for `text`, or `None` when this build/platform/config does
/// not seal (the caller then writes plaintext).
fn sealed_bytes(text: &str, seal: bool) -> Option<String> {
    if !seal {
        return None;
    }
    let blob = protect(text.as_bytes())?;
    Some(format!(
        "{ENVELOPE_PREFIX}{}",
        base64::engine::general_purpose::STANDARD.encode(blob)
    ))
}

/// Re-seal a plaintext credential file that was written before this build.
/// Best effort by design: a failure leaves the working plaintext file alone.
///
/// A file that is not valid JSON is left exactly as it is. Every caller
/// refuses to overwrite a malformed store so the user can repair it, and
/// sealing the damaged bytes would take that away (and hide what went wrong
/// behind base64).
fn migrate_in_place(path: &Path, text: &str, seal: bool) {
    if text.trim().is_empty() || serde_json::from_str::<serde_json::Value>(text).is_err() {
        return;
    }
    let Some(envelope) = sealed_bytes(text, seal) else {
        return;
    };
    match crate::config::private_atomic_write(path, envelope.as_bytes()) {
        Ok(()) => tracing::debug!("encrypted {} at rest (DPAPI, user scope)", path.display()),
        Err(e) => tracing::warn!("could not encrypt {} at rest: {e}", path.display()),
    }
}

#[cfg(windows)]
pub(crate) fn protect(plain: &[u8]) -> Option<Vec<u8>> {
    use windows_sys::Win32::Security::Cryptography::{
        CryptProtectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };
    // SAFETY: both input blobs point at live slices for the duration of the
    // call, the output blob is zeroed first and its buffer is copied out and
    // freed before returning.
    unsafe {
        let input = CRYPT_INTEGER_BLOB {
            cbData: plain.len() as u32,
            pbData: plain.as_ptr() as *mut u8,
        };
        let entropy = CRYPT_INTEGER_BLOB {
            cbData: ENTROPY.len() as u32,
            pbData: ENTROPY.as_ptr() as *mut u8,
        };
        let mut out = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: std::ptr::null_mut(),
        };
        let ok = CryptProtectData(
            &input,
            std::ptr::null(),
            &entropy,
            std::ptr::null(),
            std::ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut out,
        );
        if ok == 0 || out.pbData.is_null() {
            tracing::warn!(
                "CryptProtectData failed (error {}) - credential written unencrypted",
                windows_sys::Win32::Foundation::GetLastError()
            );
            return None;
        }
        let blob = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        windows_sys::Win32::Foundation::LocalFree(out.pbData as *mut core::ffi::c_void);
        Some(blob)
    }
}

#[cfg(windows)]
pub(crate) fn unprotect(blob: &[u8]) -> Option<Vec<u8>> {
    use windows_sys::Win32::Security::Cryptography::{
        CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };
    // SAFETY: as in `protect` - inputs outlive the call, the output buffer is
    // copied out and freed.
    unsafe {
        let input = CRYPT_INTEGER_BLOB {
            cbData: blob.len() as u32,
            pbData: blob.as_ptr() as *mut u8,
        };
        let entropy = CRYPT_INTEGER_BLOB {
            cbData: ENTROPY.len() as u32,
            pbData: ENTROPY.as_ptr() as *mut u8,
        };
        let mut out = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: std::ptr::null_mut(),
        };
        let ok = CryptUnprotectData(
            &input,
            std::ptr::null_mut(),
            &entropy,
            std::ptr::null(),
            std::ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut out,
        );
        if ok == 0 || out.pbData.is_null() {
            return None;
        }
        let plain = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        windows_sys::Win32::Foundation::LocalFree(out.pbData as *mut core::ffi::c_void);
        Some(plain)
    }
}

#[cfg(not(windows))]
pub(crate) fn protect(_plain: &[u8]) -> Option<Vec<u8>> {
    None
}

#[cfg(not(windows))]
pub(crate) fn unprotect(_blob: &[u8]) -> Option<Vec<u8>> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("nur-credential-seal-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    /// Ways the envelope can fail: ciphertext handed back as if it were the
    /// credential, a tampered or foreign blob decrypting to garbage instead
    /// of an error, a plaintext file from an older build no longer readable,
    /// a malformed store rewritten (which would defeat every caller's
    /// refusal to overwrite one).
    #[test]
    fn envelope_round_trips_and_a_damaged_one_is_an_error_not_a_credential() {
        let seal = cfg!(windows);
        let path = tmp("auth.json");
        let secret = r#"{"api_key":"PLACEHOLDER-FIXTURE-VALUE-0001"}"#;
        write_with(&path, secret, seal).unwrap();
        let on_disk = std::fs::read_to_string(&path).unwrap();
        assert_eq!(read_with(&path, seal).unwrap(), secret);

        if seal {
            assert!(on_disk.starts_with(ENVELOPE_PREFIX), "{on_disk}");
            assert!(
                !on_disk.contains("PLACEHOLDER-FIXTURE-VALUE"),
                "secret in cleartext"
            );
            assert!(is_sealed(&path));

            // A blob from another account/machine, or a flipped byte, must be
            // an error: silently returning the ciphertext would store it as
            // the user's key.
            let damaged = format!("{ENVELOPE_PREFIX}{}", "AQAAANCMnd8BFdERjHoAwE/Cl+sB");
            std::fs::write(&path, &damaged).unwrap();
            let err = read_with(&path, seal).unwrap_err();
            assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        } else {
            assert_eq!(on_disk, secret, "no sealing outside Windows");
            assert!(!is_sealed(&path));
        }

        // Written by an older build: still readable, and migrated in place
        // where sealing is available.
        let legacy = tmp("legacy.json");
        std::fs::write(&legacy, secret).unwrap();
        assert_eq!(read_with(&legacy, seal).unwrap(), secret);
        assert_eq!(is_sealed(&legacy), seal, "plaintext is re-sealed on read");

        let broken = tmp("broken.json");
        std::fs::write(&broken, b"{not-json").unwrap();
        assert_eq!(read_with(&broken, seal).unwrap(), "{not-json");
        assert_eq!(std::fs::read(&broken).unwrap(), b"{not-json");
        let _ = std::fs::remove_dir_all(broken.parent().unwrap());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
        let _ = std::fs::remove_dir_all(legacy.parent().unwrap());
    }
}
