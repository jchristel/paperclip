// crates/aconex/src/client.rs
//
// The HTTP client. Holds a reqwest client, an authenticator, and the Aconex
// base URL. For this first slice it exposes ONE method — issue an
// authenticated GET and return the response body as a String — so we can prove
// the whole chain (build client → attach auth headers → GET → read body) end
// to end before introducing typed XML parsing.
//
// Async notes for a C# dev:
//   async fn        ≈  async Task<T>     (returns a future, doesn't block)
//   .await          ≈  await            (suspend until the future completes)
//   reqwest::Client ≈  HttpClient        (reuse one instance; it pools connections)
// The big difference from C#: Rust needs an external runtime (tokio) to drive
// the futures. That's wired up in the CLI via #[tokio::main], not here.

use reqwest::Client as HttpClient;

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
    /// This is deliberately minimal for the first slice: no XML parsing yet,
    /// no per-endpoint typing. It proves auth + transport work. The `async fn`
    /// means callers must `.await` it from inside an async context.
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
        // `error_for_status_ref` checks the status without consuming the
        // response, so we could still read the body on success below.
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
}
