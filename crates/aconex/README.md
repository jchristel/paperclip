# aconex

A from-scratch async Rust client library for the [Aconex](https://www.aconex.com/)
cloud document-management platform. Part of the `paperclip` workspace; designed
to be usable on its own (and publishable separately) later.

## Status

Working end to end for its current endpoints. The crate authenticates, lists
and resolves projects, and runs typed, paginated document searches against a
live Aconex instance.

| Area | State |
|---|---|
| Basic auth (username/password/app-key) | ✅ working |
| OAuth | 🔲 stubbed (implements the same trait, returns "not implemented") |
| Project list / lookup by short name | ✅ working, typed |
| Document register search | ✅ working, typed, auto-paginated |
| Document download / upload | 🔲 planned |
| Mail | 🔲 planned |

## Design principles

- **Typed, not dict-fishing.** API responses deserialize into explicit structs
  via `serde` + `quick-xml`, not loose maps.
- **Explicit errors.** A dedicated `AconexError` enum (via `thiserror`) rather
  than panics. The crate never leaks dependency error types (`reqwest`,
  `quick-xml`) into its public API — callers only ever match on `AconexError`.
- **A `Client` value, not a god-object.** State (HTTP client, authenticator,
  base URL) lives on a `Client`, with methods grouped by API area across
  modules (auth, projects, search). Methods are added to `Client` via multiple
  `impl` blocks so each file stays focused.
- **Credentials injected, never owned.** The library receives credentials from
  the caller; it never touches OS secret storage. The CLI reads them from
  Windows Credential Manager and passes them in.

## Module layout

```
crates/aconex/src/
├── lib.rs        # module declarations + curated re-exports
├── error.rs      # AconexError enum + crate Result alias
├── auth.rs       # Header, Authenticator trait, BasicAuth, OAuth stub
├── client.rs     # Client: reqwest + authenticator + base URL; get_text
├── projects.rs   # ProjectResults/Project model; get_projects, get_project
└── search.rs     # Document model; search_documents (auto-paginating)
```

## Dependencies

| Crate | Purpose |
|---|---|
| `reqwest` (0.13) | async HTTP client (rustls TLS by default) |
| `tokio` (1) | async runtime that drives the futures |
| `quick-xml` (serialize) | XML → typed structs via serde |
| `serde` (derive) | deserialization derives |
| `base64` | HTTP Basic credential encoding |
| `thiserror` | the error enum boilerplate |

## Authentication

Two mechanisms behind one trait (`Authenticator`), whose single job is to
produce the request's auth headers:

1. **Basic auth** — `Authorization: Basic <base64(user:pass)>` plus the
   `X-Application-Key` header, on every request. Built and working.
2. **OAuth** — a stub implementing the same trait. It returns a "not
   implemented" error today, so the rest of the crate is already written
   against the trait; a real OAuth implementation slots in later with no
   downstream changes.

Because the `Client` holds a `Box<dyn Authenticator>`, a client built with
Basic auth and one built with OAuth are the same type — the auth choice is
hidden behind the trait, and every endpoint is auth-agnostic.

## Usage sketch

```rust
use aconex::{BasicAuth, Client};

# async fn demo() -> aconex::Result<()> {
// Credentials are injected by the caller (e.g. read from Credential Manager).
let auth = BasicAuth::new("username", "password", "app-key")?;
let client = Client::new(auth); // defaults to the au1 instance

// Resolve a project by its short name, then search its register.
if let Some(project) = client.get_project("RHH").await? {
    let docs = client.search_documents(&project, "RHH-HDR-AR-DRG-A130001").await?;
    for d in docs {
        println!("{:?} — {:?}", d.document_number, d.title);
    }
}
# Ok(())
# }
```

`Client::with_base_url(auth, url)` targets a non-default Aconex region or a test
server.

## Lessons baked into the code

The Aconex register API has several quirks that are documented inline where they
matter, learned from real responses:

- **Request vs response field vocabularies differ.** You request fields by
  lowercase names (`docno`, `title`, `revisiondate`) but receive PascalCase
  child elements (`<DocumentNumber>`, `<Title>`, `<RevisionDate>`). Feeding a
  response element name back as a request field returns HTTP 400. `search.rs`
  documents the mapping in its `RETURN_FIELDS` constant.
- **Documents carry `DocumentId` as an attribute and requested fields as child
  elements.** The `Document` struct models this with `#[serde(rename = "@...")]`
  for the id and plain renames for the children. Every field but the id is
  `Option`, since a field appears only if requested and present.
- **Leading wildcards are rejected.** A query starting with `*` or `?` makes
  Aconex return HTTP 500 (matching its web UI). `search_documents` guards
  against this client-side with a clear error and no wasted round-trip.
- **`page_size` is constrained.** 500 is known-good; some smaller values (e.g.
  10) return HTTP 400 on the register endpoint.

The `paperclip aconex diag` command group exists to discover quirks like these
against a live instance — see the CLI README.

## Tests

Unit tests live beside the code (`#[cfg(test)] mod tests`) and parse real
captured response XML — no network needed. Run with `cargo test -p aconex`.
Covered: project parsing (single, multiple, empty, unmodelled-field tolerance)
and document search parsing (full fields, missing fields → `None`, empty
results).

## Roadmap

- [ ] Split a dedicated `Parse` error variant out of `Http` (XML parse failures
      currently surface as `Http`).
- [ ] Document download and upload/register.
- [ ] Mail creation and forwarding.
- [ ] Optional dynamic field selection driven by the register schema (the
      `diag schema` command already exposes the schema for discovery).
- [ ] Request throttling to respect Aconex rate limits.
