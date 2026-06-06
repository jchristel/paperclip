# aconex

A from-scratch Rust client library for the [Aconex](https://www.aconex.com/)
cloud document-management platform. Part of the `paperclip` workspace; intended
to be usable on its own (and publishable separately) once it matures.

> **Status: early stub.** The crate compiles and reserves its place in the
> workspace, but contains no working client yet. The sections below describe the
> *planned* shape so the design is on record before the code exists.

---

## Why this crate exists

The `paperclip` CLI will eventually push assembled binders (and their metadata)
up to Aconex. That needs an HTTP client for the Aconex REST API: authentication,
project lookup, document search, upload, and mail. Keeping it as a separate
library crate — rather than folding it into the CLI — means:

- The CLI stays focused on PDF assembly.
- The client has no dependency on CLI-specific concerns (Windows Credential
  Manager, clap, etc.). Credentials are *injected in* by the caller.
- It can be split out and published independently later with no restructuring.

---

## Design principles

- **Typed, not dict-fishing.** API responses deserialize into explicit Rust
  structs via `serde`, rather than being navigated as loose maps.
- **Explicit errors.** A dedicated error type (likely via `thiserror`) instead of
  panics/assertions, so callers can handle auth failures, throttling, and
  malformed responses distinctly.
- **A `Client` value, not a god-object.** State (HTTP client, credentials,
  selected project) lives on a `Client` struct with focused methods, organised
  into modules by API area (auth, projects, documents, mail) rather than one
  flat surface.
- **Credentials injected, never owned.** The library receives credentials from
  the caller (the CLI reads them from Credential Manager) and never touches
  OS-specific secret storage itself.

---

## Authentication

Two mechanisms, built in this order:

1. **Basic auth (username + password)** — the first slice of work. Aconex accepts
   an `Authorization: Basic <base64(user:pass)>` header plus an application-key
   header on every request. Simple and sufficient to get end-to-end calls
   working.
2. **OAuth** — *stubbed for later.* The intended design is an auth abstraction
   (a trait) with one job: attach the right auth headers to an outgoing request.
   Basic auth implements it first; an OAuth implementation slots into the same
   seam later without changing any calling code. The OAuth type will exist as a
   placeholder that returns a "not implemented" error until it's built, so the
   extension point is visible and ready.

---

## Planned surface (roadmap)

Built incrementally, smallest useful slice first:

- [ ] Auth abstraction + Basic auth implementation
- [ ] OAuth stub implementing the same auth trait (returns not-implemented)
- [ ] `Client` construction (HTTP client + injected credentials)
- [ ] `get_projects` / project lookup by short name — the first end-to-end call
- [ ] Document register search
- [ ] Document download
- [ ] Document upload / register
- [ ] Mail creation and forwarding
- [ ] Typed error enum covering auth, throttling, and parse failures
- [ ] Request throttling to respect Aconex rate limits

---

## Dependencies

None yet. Planned additions when the first client code lands include an HTTP
client (`reqwest`), `serde` for deserialisation, and an XML parser for the
XML-based API responses. These will be added in the session that first needs
them, not before.

---

## Usage

Nothing to call yet. Once the first slice exists, this section will show
constructing a `Client` with injected credentials and listing projects.
