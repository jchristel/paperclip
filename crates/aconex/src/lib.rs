// crates/aconex/src/lib.rs
//
// Library crate for talking to the Aconex API.
// `lib.rs` is the crate root for a library — the equivalent of `main.rs`
// for a binary. Whatever is marked `pub` here is what other crates (like
// paperclip-cli) can see, similar to `public` types in a C# class library.

// --- Modules -------------------------------------------------------------
// `mod` declares a module and tells the compiler to pull in the matching file.
// `pub mod` makes the module itself reachable from outside the crate.
pub mod auth;
pub mod client;
pub mod error;
pub mod projects;

// --- Re-exports ----------------------------------------------------------
// `pub use` lifts the commonly-used types up to the crate root so callers can
// write the shorter `aconex::BasicAuth` instead of `aconex::auth::BasicAuth`.
// This is the Rust equivalent of curating what a C# namespace surfaces at its
// top level. The module paths above still work too; this is just convenience
// for the names people reach for most.
pub use auth::{Authenticator, BasicAuth, Header, OAuth};
pub use client::Client;
pub use error::{AconexError, Result};
pub use projects::{Project, ProjectResults};
