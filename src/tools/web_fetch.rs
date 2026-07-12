//! Web fetch with real SSRF protection: every hop's host is resolved and all
//! addresses must be public. Redirects are followed manually so each hop is
//! re-validated. Bodies are streamed and capped, never fully buffered.

use super::{arg_str, arg_u64, Tool, ToolContext};
use crate::error::{MuseError, Result};
use serde_json::Value;
use std::io::Read;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, ToSocketAddrs};

pub struct WebFetch;

const MAX_REDIRECTS: usize = 5;

impl Tool for WebFetch {
    fn name(&self) -> &str {
        "web_fetch"
    }

    fn description(&self) -> &str {
        "Fetch a public HTTP(S) URL and return text content (HTML/JSON/plain). \
         Max 500KB. Use for docs and APIs — not for authenticated/private resources."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "url": {"type": "string"},
                "max_bytes": {"type": "integer", "default": 200000}
            },
            "required": ["url"]
        })
    }

    fn execute(&self, args: &Value, _ctx: &ToolContext) -> Result<String> {
        let url = arg_str(args, "url")?;
        let max = arg_u64(args, "max_bytes").unwrap_or(200_000) as usize;
        let max = max.min(500_000);

        let mut current = url.clone();
        for _hop in 0..=MAX_REDIRECTS {
            let (parsed, pin) = validate_public_url(&current)?;

            // Pin the validated IP so DNS rebinding between the check and the
            // connect cannot redirect the request to a private address.
            let mut builder = reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .user_agent(format!("meta-cli/{}", env!("CARGO_PKG_VERSION")))
                // Redirects are followed manually below so each hop is re-checked.
                .redirect(reqwest::redirect::Policy::none());
            if let Some((host, addr)) = &pin {
                builder = builder.resolve(host, *addr);
            }
            let client = builder.build().map_err(|e| MuseError::Tool(e.to_string()))?;

            let resp = client
                .get(parsed)
                .send()
                .map_err(|e| MuseError::Tool(format!("fetch failed: {e}")))?;

            let status = resp.status();
            if status.is_redirection() {
                let loc = resp
                    .headers()
                    .get(reqwest::header::LOCATION)
                    .and_then(|v| v.to_str().ok())
                    .ok_or_else(|| MuseError::Tool("redirect without Location".into()))?;
                // Resolve relative redirects against the current URL.
                let base = reqwest::Url::parse(&current)
                    .map_err(|e| MuseError::Tool(format!("bad url: {e}")))?;
                let next = base
                    .join(loc)
                    .map_err(|e| MuseError::Tool(format!("bad redirect: {e}")))?;
                current = next.to_string();
                continue;
            }

            let ctype = resp
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string();

            // Stream-capped read: never buffer more than max+1 bytes.
            let mut buf = Vec::with_capacity(max.min(65_536));
            resp.take(max as u64 + 1)
                .read_to_end(&mut buf)
                .map_err(|e| MuseError::Tool(format!("read body: {e}")))?;
            let truncated = buf.len() > max;
            let slice = if truncated { &buf[..max] } else { &buf[..] };
            let text = String::from_utf8_lossy(slice);

            let mut out = format!("url: {current}\nstatus: {status}\ncontent-type: {ctype}\n\n");
            out.push_str(&text);
            if truncated {
                out.push_str(&format!("\n\n[truncated at {max} bytes]"));
            }
            return Ok(out);
        }
        Err(MuseError::Tool(format!(
            "too many redirects (>{MAX_REDIRECTS}): {url}"
        )))
    }
}

/// Parse the URL, resolve its host, and require every address to be public.
/// For hostname URLs, also returns `(host, addr)` to pin for the connection.
fn validate_public_url(
    url: &str,
) -> Result<(reqwest::Url, Option<(String, std::net::SocketAddr)>)> {
    let parsed =
        reqwest::Url::parse(url).map_err(|e| MuseError::Tool(format!("bad url: {e}")))?;
    match parsed.scheme() {
        "http" | "https" => {}
        s => return Err(MuseError::Tool(format!("refused scheme: {s}"))),
    }
    let host: String = parsed
        .host_str()
        .ok_or_else(|| MuseError::Tool("url has no host".into()))?
        .to_string();
    let host = host.as_str();

    // Cloud metadata hostnames.
    let hl = host.to_ascii_lowercase();
    if hl == "localhost"
        || hl.ends_with(".localhost")
        || hl.ends_with(".local")
        || hl.ends_with(".internal")
        || hl.contains("metadata")
    {
        return Err(MuseError::Tool(format!("refused local/metadata host: {host}")));
    }

    // Literal IP → check directly (no pin needed); hostname → resolve, check
    // every record, and pin the first validated address for the connection.
    if let Ok(ip) = hl.trim_matches(['[', ']']).parse::<IpAddr>() {
        if !ip_is_public(ip) {
            return Err(MuseError::Tool(format!(
                "refused non-public address: {host} → {ip}"
            )));
        }
        return Ok((parsed, None));
    }

    let port = parsed.port_or_known_default().unwrap_or(443);
    let sockaddrs: Vec<std::net::SocketAddr> = (host, port)
        .to_socket_addrs()
        .map_err(|e| MuseError::Tool(format!("dns resolve {host}: {e}")))?
        .collect();
    if sockaddrs.is_empty() {
        return Err(MuseError::Tool(format!("dns: no addresses for {host}")));
    }
    for sa in &sockaddrs {
        if !ip_is_public(sa.ip()) {
            return Err(MuseError::Tool(format!(
                "refused non-public address: {host} → {}",
                sa.ip()
            )));
        }
    }
    Ok((parsed, Some((host.to_string(), sockaddrs[0]))))
}

fn ip_is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => ipv4_is_public(v4),
        IpAddr::V6(v6) => ipv6_is_public(v6),
    }
}

fn ipv4_is_public(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    !(ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.is_multicast()
        || ip.is_documentation()
        || o[0] == 100 && (o[1] & 0xC0) == 64 // CGNAT 100.64.0.0/10
        || o[0] == 192 && o[1] == 0 && o[2] == 0 // IETF protocol 192.0.0.0/24
        || o[0] == 198 && (o[1] & 0xFE) == 18 // benchmarking 198.18.0.0/15
        || o[0] >= 240) // reserved 240.0.0.0/4
}

fn ipv6_is_public(ip: Ipv6Addr) -> bool {
    // IPv4-mapped/compatible → judge the embedded v4.
    if let Some(v4) = ip.to_ipv4_mapped() {
        return ipv4_is_public(v4);
    }
    let seg = ip.segments();
    !(ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        || (seg[0] & 0xFE00) == 0xFC00 // unique-local fc00::/7
        || (seg[0] & 0xFFC0) == 0xFE80 // link-local fe80::/10
        || (seg[0] == 0x2001 && seg[1] == 0x0DB8) // documentation
        || (seg[0] == 0x0064 && seg[1] == 0xFF9B)) // NAT64 well-known prefix
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_private_and_local() {
        for bad in [
            "http://127.0.0.1/x",
            "http://127.1/x",
            "http://localhost:8080/",
            "http://10.0.0.5/",
            "http://172.16.3.4/",
            "http://192.168.1.1/admin",
            "http://169.254.169.254/latest/meta-data/",
            "http://100.64.0.1/",
            "http://[::1]/",
            "http://[fd00::1]/",
            "http://[fe80::1]/",
            "http://[::ffff:10.0.0.1]/",
            "http://2130706433/", // decimal 127.0.0.1
            "file:///etc/passwd",
            "http://metadata.google.internal/",
        ] {
            assert!(validate_public_url(bad).is_err(), "should refuse {bad}");
        }
    }

    #[test]
    fn allows_public_literals() {
        assert!(validate_public_url("https://1.1.1.1/").is_ok());
        assert!(validate_public_url("http://93.184.216.34/").is_ok());
    }
}
