// crates/aconex/src/client.rs
//
// The HTTP client. Holds a reqwest client, an authenticator, and the Aconex
// base URL. It exposes authenticated GETs in two flavours: `get_text` (body as
// a String, for XML endpoints) and `get_bytes` (body as raw bytes, for binary
// downloads like PDFs/DWGs).
//
// Async notes for a C# dev:
//   async fn        ≈  async Task<T>     (returns a future, doesn't block)
//   .await          ≈  await            (suspend until the future completes)
//   reqwest::Client ≈  HttpClient        (reuse one instance; it pools connections)
// The big difference from C#: Rust needs an external runtime (tokio) to drive
// the futures. That's wired up in the CLI via #[tokio::main], not here.

use reqwest::Client as HttpClient;
use reqwest::header::CONTENT_DISPOSITION;

use crate::auth::Authenticator;
use crate::error::{AconexError, Result};

/// The default Aconex instance. `au1` is the Australian region; other regions
/// exist (e.g. other geographic instances). We keep it overridable via
/// `Client::with_base_url` rather than hardcoding it as a constant everywhere.
const DEFAULT_BASE_URL: &str = "https://au1.aconex.com";

/// An authenticated Aconex HTTP client.
///
/// `Box<dyn Authenticator>` is the interesting bit: it holds *some* type that
/// implements `Authenticator`, chosen at runtime, without the Client needing to
/// know which one. `dyn Trait` is Rust's dynamic dispatch — the equivalent of
/// storing an `IAuthenticator` reference in C# and calling through the
/// interface. `Box<…>` puts it on the heap because the concrete size isn't
/// known at compile time. So a Client built with BasicAuth and one built with
/// OAuth are the same type — the auth choice is hidden behind the trait.
pub struct Client {
    http: HttpClient,
    auth: Box<dyn Authenticator>,
    base_url: String,
}

/// The result of a binary GET: the body bytes plus, if the server sent one,
/// the filename it suggested via the Content-Disposition header.
///
/// `suggested_filename` is `Option` because the header may be absent or in a
/// form we don't recognise. The download method doesn't strictly need it (the
/// caller supplies the path), but exposing it keeps `get_bytes` reusable.
#[derive(Debug, Clone)]
pub struct DownloadResponse {
    pub bytes: Vec<u8>,
    pub suggested_filename: Option<String>,
}

impl Client {
    /// Builds a client against the default (au1) Aconex instance.
    ///
    /// Takes any `A: Authenticator + 'static` and boxes it. The `'static`
    /// bound says the authenticator owns its data (no borrowed references that
    /// could dangle) — true for BasicAuth, which owns its credential Strings.
    pub fn new<A: Authenticator + 'static>(auth: A) -> Self {
        Self::with_base_url(auth, DEFAULT_BASE_URL)
    }

    /// Builds a client against an explicit base URL (other regions, or a test
    /// server). Trailing slashes are trimmed so path-joining is predictable.
    pub fn with_base_url<A: Authenticator + 'static>(
        auth: A,
        base_url: impl Into<String>,
    ) -> Self {
        let base_url = base_url.into().trim_end_matches('/').to_string();
        Client {
            http: HttpClient::new(),
            auth: Box::new(auth),
            base_url,
        }
    }

    /// Issues an authenticated GET to `path` (e.g. "/api/projects/") and
    /// returns the raw response body as a String.
    ///
    /// This is deliberately minimal: no XML parsing yet, no per-endpoint
    /// typing. It proves auth + transport work. The `async fn` means callers
    /// must `.await` it from inside an async context.
    pub async fn get_text(&self, path: &str) -> Result<String> {
        // Build the absolute URL. `path` is expected to start with '/'.
        let url = format!("{}{}", self.base_url, path);

        // Ask the authenticator for its headers. `?` propagates an auth error
        // (e.g. the OAuth stub, or a future validation failure) to the caller.
        let auth_headers = self.auth.auth_headers()?;

        // Start building the request, then attach each auth header. reqwest's
        // builder is fluent (returns Self), so we fold the headers on in a loop.
        let mut request = self.http.get(&url);
        for header in auth_headers {
            request = request.header(header.name, header.value);
        }

        // Send it. `.await` suspends until the response headers arrive.
        // `map_err` converts reqwest's own error into our AconexError — a
        // library hands back ITS error type, not a dependency's, so callers
        // don't have to know we use reqwest underneath.
        let response = request
            .send()
            .await
            .map_err(|e| AconexError::Http(e.to_string()))?;

        // Treat any non-2xx as an error, capturing the status for the message.
        let status = response.status();
        if !status.is_success() {
            return Err(AconexError::Http(format!(
                "GET {} returned HTTP {}",
                url, status
            )));
        }

        // Read the whole body as text. Also async — the body may arrive in
        // chunks, so reading it is itself an awaitable operation.
        let body = response
            .text()
            .await
            .map_err(|e| AconexError::Http(format!("reading body of {}: {}", url, e)))?;

        Ok(body)
    }

    /// Issues an authenticated GET to `path` and returns the raw response body
    /// as BYTES, together with the server's suggested filename (parsed out of
    /// the Content-Disposition header, if present).
    ///
    /// This is the binary sibling of `get_text`. Downloads (PDFs, DWGs) aren't
    /// valid UTF-8, so we can't go through `String`; we keep the bytes raw.
    /// The duplicated auth/URL/status plumbing is deliberate for now — once a
    /// third caller appears we can factor the common "build + send" step out.
    pub async fn get_bytes(&self, path: &str) -> Result<DownloadResponse> {
        // Same URL-building and auth-header attaching as get_text.
        let url = format!("{}{}", self.base_url, path);
        let auth_headers = self.auth.auth_headers()?;

        let mut request = self.http.get(&url);
        for header in auth_headers {
            request = request.header(header.name, header.value);
        }

        let response = request
            .send()
            .await
            .map_err(|e| AconexError::Http(e.to_string()))?;

        let status = response.status();
        if !status.is_success() {
            return Err(AconexError::Http(format!(
                "GET {} returned HTTP {}",
                url, status
            )));
        }

        // Pull the suggested filename out of Content-Disposition BEFORE we
        // consume the response body. `response.headers()` borrows the response;
        // `.bytes()` below takes ownership, so the order matters — read the
        // header first, into an owned Option<String>, then read the body.
        let suggested_filename = response
            .headers()
            .get(CONTENT_DISPOSITION)            // Option<&HeaderValue>
            .and_then(|v| v.to_str().ok())       // Option<&str> (None if non-ASCII)
            .and_then(parse_content_disposition_filename); // Option<String>

        // Read the whole body as raw bytes. `.bytes()` returns reqwest's `Bytes`
        // (a cheap-to-clone byte buffer); `.to_vec()` copies it into a plain
        // Vec<u8> so nothing reqwest-specific leaks out of our API.
        let bytes = response
            .bytes()
            .await
            .map_err(|e| AconexError::Http(format!("reading body of {}: {}", url, e)))?
            .to_vec();

        Ok(DownloadResponse { bytes, suggested_filename })
    }
}

/// Extracts the filename from a Content-Disposition header value.
///
/// Aconex sends it in the RFC 5987 extended form, e.g.
///   attachment; filename*=UTF-8''RHH-HDR-AR-DRG-A130001-%5BB%5D.pdf
/// The Python read this as `.split("''")[1]` then URL-decoded it. We mirror
/// that: take everything after the `''`, then percent-decode it. We also
/// handle the plain `filename="foo.pdf"` form as a fallback.
///
/// Returns None if no filename can be found — the caller then falls back to
/// its own path, so a missing header is never fatal.
fn parse_content_disposition_filename(header: &str) -> Option<String> {
    // Preferred: the RFC 5987 `filename*=charset''<percent-encoded>` form.
    // `split_once("''")` gives us everything after the two quotes in one step
    // (it splits on the FIRST occurrence and returns the two halves).
    if let Some((_, encoded)) = header.split_once("''") {
        // `encoded` may have a trailing `; ...` if more params follow; cut it.
        let encoded = encoded.split(';').next().unwrap_or(encoded).trim();
        return Some(percent_decode(encoded));
    }

    // Fallback: the plain `filename="foo.pdf"` form.
    if let Some((_, rest)) = header.split_once("filename=") {
        let name = rest
            .split(';')
            .next()
            .unwrap_or(rest)
            .trim()
            .trim_matches('"'); // strip surrounding quotes if present
        if !name.is_empty() {
            return Some(name.to_string());
        }
    }

    None
}

/// Minimal percent-decoder (reverse of the urlencode in search.rs). Turns
/// "%5BB%5D" back into "[B]". Bytes that aren't a valid `%XX` escape are
/// passed through unchanged, so ordinary characters survive untouched.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            // Try to read the next two chars as a hex byte.
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            if let (Some(hi), Some(lo)) = (hi, lo) {
                out.push((hi * 16 + lo) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    // The decoded bytes are UTF-8 (Aconex declares UTF-8); lossy keeps us safe
    // if anything unexpected slips through rather than erroring.
    String::from_utf8_lossy(&out).into_owned()
}

// --- Tests ---------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rfc5987_extended_filename() {
        let header = "attachment; filename*=UTF-8''RHH-HDR-AR-DRG-A130001-%5BB%5D.pdf";
        assert_eq!(
            parse_content_disposition_filename(header).as_deref(),
            Some("RHH-HDR-AR-DRG-A130001-[B].pdf")
        );
    }

    #[test]
    fn parses_plain_quoted_filename() {
        let header = r#"attachment; filename="report.pdf""#;
        assert_eq!(
            parse_content_disposition_filename(header).as_deref(),
            Some("report.pdf")
        );
    }

    #[test]
    fn missing_filename_is_none() {
        assert!(parse_content_disposition_filename("attachment").is_none());
    }

    #[test]
    fn percent_decode_passes_plain_text_through() {
        assert_eq!(percent_decode("plain.pdf"), "plain.pdf");
        assert_eq!(percent_decode("a%20b%5Bc%5D"), "a b[c]");
    }
}