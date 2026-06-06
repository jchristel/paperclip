// crates/aconex/src/auth.rs
//
// Authentication for the Aconex API.
//
// Aconex Basic auth needs two headers on every request:
//   Authorization: Basic <base64("username:password")>
//   X-Application-Key: <app key>
// So the auth layer's whole job is "produce those headers." We model that as a
// trait (≈ a C# interface) with one method. BasicAuth implements it now; an
// OAuth implementation slots into the same seam later without changing any
// calling code.
//
// The trait deliberately returns plain header data (no reqwest types), so the
// auth layer stays independent of whichever HTTP client we add later, and so
// endpoints can take these headers and append their own (Accept,
// Content-Type, X-On-Behalf-Of, ...) on top — which several Aconex calls do.

use base64::Engine; // brings the `.encode()` method into scope (see encode call below)

use crate::error::{AconexError, Result};

// --- Header --------------------------------------------------------------

/// A single HTTP header as a name/value pair.
///
/// We use a named struct rather than a bare `(String, String)` tuple so the
/// two strings can't be confused or swapped, and so there's ONE place — the
/// `new` constructor — to add validation later (e.g. rejecting newlines to
/// prevent header injection) without touching any caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub name: String,
    pub value: String,
}

impl Header {
    /// Builds a header. Returns `Result` so that when we add validation later,
    /// callers already handle the failure path — adding the checks won't be a
    /// breaking change. Today it cannot actually fail, but the signature is
    /// future-proofed.
    ///
    /// `impl Into<String>` lets callers pass either a `&str` or an owned
    /// `String` — the same ergonomic trick as a C# overload taking `string`.
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Result<Self> {
        let name = name.into();
        let value = value.into();

        // Placeholder for future validation. Example of what will live here:
        //   if value.contains(['\r', '\n']) {
        //       return Err(AconexError::InvalidHeader { ... });
        //   }
        // Left out deliberately for now; the point is the chokepoint exists.

        Ok(Header { name, value })
    }
}

// --- The Authenticator trait ---------------------------------------------

/// Something that can produce the authentication headers for a request.
///
/// A `trait` is Rust's interface: it declares method signatures that
/// implementing types must provide. Any type that implements `Authenticator`
/// can be used wherever auth is needed, so the HTTP client (built later) will
/// accept "some Authenticator" without caring whether it's Basic or OAuth.
pub trait Authenticator {
    /// Returns the headers to attach to an outgoing request.
    ///
    /// Returns a `Vec<Header>` (owned list) rather than mutating a request, so
    /// the caller starts from these and adds its own request-specific headers.
    /// `&self` = takes the authenticator by shared reference (read-only), like
    /// a C# instance method that doesn't mutate state.
    fn auth_headers(&self) -> Result<Vec<Header>>;
}

// --- Basic auth ----------------------------------------------------------

/// Username + password + application key, encoded as HTTP Basic auth.
///
/// This owns its credential strings. The CLI reads them from settings +
/// Credential Manager and hands them in; this type never touches TOML or
/// Windows APIs itself — credentials are injected, not fetched.
#[derive(Debug, Clone)]
pub struct BasicAuth {
    username: String,
    password: String,
    app_key: String,
}

impl BasicAuth {
    /// Builds a `BasicAuth`, rejecting blank inputs up front so a misconfigured
    /// run fails with a clear "missing credential: X" rather than producing a
    /// header that the server silently rejects later.
    pub fn new(
        username: impl Into<String>,
        password: impl Into<String>,
        app_key: impl Into<String>,
    ) -> Result<Self> {
        let username = username.into();
        let password = password.into();
        let app_key = app_key.into();

        // Guard each field. `.is_empty()` on a String is like
        // string.IsNullOrEmpty in C# (we already know it's non-null here).
        if username.is_empty() {
            return Err(AconexError::MissingCredential("username".into()));
        }
        if password.is_empty() {
            return Err(AconexError::MissingCredential("password".into()));
        }
        if app_key.is_empty() {
            return Err(AconexError::MissingCredential("app_key".into()));
        }

        Ok(BasicAuth { username, password, app_key })
    }

    /// Builds the `Basic <base64>` value for the Authorization header.
    /// Private: it's an implementation detail of `auth_headers`.
    fn authorization_value(&self) -> String {
        // Aconex (and the HTTP Basic spec) want base64 of "username:password".
        let raw = format!("{}:{}", self.username, self.password);

        // base64 has multiple alphabets; STANDARD is the ordinary one used by
        // HTTP Basic. `.encode()` takes bytes and returns the base64 String.
        let encoded = base64::engine::general_purpose::STANDARD.encode(raw.as_bytes());

        format!("Basic {}", encoded)
    }
}

/// `impl Trait for Type` is how a type declares it satisfies an interface —
/// like `class BasicAuth : IAuthenticator` in C#.
impl Authenticator for BasicAuth {
    fn auth_headers(&self) -> Result<Vec<Header>> {
        // Two headers, every request. `?` propagates a Header::new failure if
        // validation is ever added; today these always succeed.
        Ok(vec![
            Header::new("Authorization", self.authorization_value())?,
            Header::new("X-Application-Key", self.app_key.clone())?,
        ])
    }
}

// --- OAuth (stub) --------------------------------------------------------

/// Placeholder for OAuth. It implements the SAME `Authenticator` trait, so the
/// rest of the crate can already be written against the trait — when OAuth is
/// built for real, nothing downstream changes. For now any attempt to use it
/// fails loudly rather than pretending to work.
///
/// The fields it'll need (token, expiry, refresh) aren't modelled yet; this is
/// intentionally just enough to hold the seam open.
#[derive(Debug, Clone, Default)]
pub struct OAuth {
    // e.g. access_token: String, expires_at: ..., refresh_token: String
}

impl Authenticator for OAuth {
    fn auth_headers(&self) -> Result<Vec<Header>> {
        // Honest failure: this path isn't implemented. Reusing MissingCredential
        // keeps the error set small for now; we'll likely add a dedicated
        // `NotImplemented` variant when OAuth work actually starts.
        Err(AconexError::MissingCredential(
            "OAuth is not implemented yet — use BasicAuth".into(),
        ))
    }
}

// --- Tests ---------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_auth_produces_two_headers() {
        let auth = BasicAuth::new("alice", "s3cret", "my-app-key").unwrap();
        let headers = auth.auth_headers().unwrap();

        assert_eq!(headers.len(), 2);
        assert_eq!(headers[0].name, "Authorization");
        assert_eq!(headers[1].name, "X-Application-Key");
        assert_eq!(headers[1].value, "my-app-key");
    }

    #[test]
    fn basic_auth_encodes_username_password() {
        let auth = BasicAuth::new("alice", "s3cret", "k").unwrap();
        let headers = auth.auth_headers().unwrap();

        // base64("alice:s3cret") == "YWxpY2U6czNjcmV0"
        assert_eq!(headers[0].value, "Basic YWxpY2U6czNjcmV0");
    }

    #[test]
    fn blank_credentials_are_rejected() {
        // Empty password should fail with MissingCredential.
        let err = BasicAuth::new("alice", "", "k").unwrap_err();
        match err {
            AconexError::MissingCredential(what) => assert_eq!(what, "password"),
            other => panic!("expected MissingCredential, got {:?}", other),
        }
    }

    #[test]
    fn oauth_stub_refuses() {
        let err = OAuth::default().auth_headers().unwrap_err();
        // Just confirm it errors rather than silently returning headers.
        assert!(matches!(err, AconexError::MissingCredential(_)));
    }
}
