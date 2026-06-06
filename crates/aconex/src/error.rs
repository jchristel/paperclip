// crates/aconex/src/error.rs
//
// The crate's error type. A *library* defines a specific, matchable error type
// (via `thiserror`) so callers can react differently to different failures —
// unlike the CLI, which uses `anyhow` to blur everything into one type for a
// top-level `main`. Rule of thumb: thiserror in libraries, anyhow in apps.
//
// `thiserror` is a derive macro: you write an enum, annotate each variant with
// the message it should print, and the macro generates the boilerplate that
// makes the enum behave as a proper std::error::Error. In C# terms it's like
// declaring a set of exception subclasses, except they're variants of one enum
// the caller can `match` on exhaustively.

use thiserror::Error;

/// Everything that can go wrong in the aconex crate.
///
/// `#[derive(Error)]` wires up the std error trait. `#[derive(Debug)]` is
/// required because the error trait demands it (so errors can always be
/// printed with `{:?}`).
///
/// As the crate grows we'll add variants (network failures, XML parse errors,
/// HTTP status errors, throttling). For the auth slice we only need a couple.
#[derive(Debug, Error)]
pub enum AconexError {
    /// A required credential was missing or blank when building an
    /// authenticator. e.g. the user ran the tool before `config set --password`.
    ///
    /// The `{0}` in the message string pulls in the first (and only) field of
    /// this variant — the name of the missing piece. So the printed message
    /// reads like "missing credential: password".
    #[error("missing credential: {0}")]
    MissingCredential(String),

    /// A header name or value was rejected (e.g. it contained characters that
    /// aren't legal in an HTTP header). Not produced yet — the `Header` type
    /// is currently permissive — but the variant exists so the validating
    /// constructor has somewhere to send failures later without a breaking
    /// change to this enum.
    #[error("invalid header {name}: {reason}")]
    InvalidHeader { name: String, reason: String },
}

/// A crate-local Result alias so signatures read `Result<T>` instead of
/// `Result<T, AconexError>` everywhere. This is the same convention the std
/// library and most crates use (e.g. `std::io::Result`). It's purely
/// ergonomic — `Result<T>` here always means "T or an AconexError".
pub type Result<T> = std::result::Result<T, AconexError>;
