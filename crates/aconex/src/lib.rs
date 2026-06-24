// crates/aconex/src/lib.rs
//
// Library crate for talking to the Aconex API.
// `lib.rs` is the crate root for a library — the equivalent of `main.rs`
// for a binary. Whatever is marked `pub` here is what other crates (like
// paperclip-cli) can see, similar to `public` types in a C# class library.

// --- Modules -------------------------------------------------------------
pub mod auth;
pub mod client;
pub mod download;
pub mod error;
pub mod projects;
pub mod search;

// --- Re-exports ----------------------------------------------------------
// `pub use` lifts the commonly-used types up to the crate root so callers can
// write the shorter `aconex::BasicAuth` instead of `aconex::auth::BasicAuth`.
pub use auth::{Authenticator, BasicAuth, Header, OAuth};
pub use client::{Client, DownloadResponse};
pub use error::{AconexError, Result};
pub use projects::{Project, ProjectResults};
pub use search::Document;