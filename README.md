# Paperclip

A Rust workspace containing a Windows CLI tool for assembling PDFs into named
binders, plus a (work-in-progress) client library for the Aconex cloud document
platform.

## Workspace layout

```
paperclip/                      workspace root (this file)
├── Cargo.toml                  workspace manifest — groups the member crates
├── install.ps1                 per-user installer for the CLI (no admin needed)
└── crates/
    ├── paperclip-cli/          the binder CLI  →  produces paperclip.exe
    │   └── README.md           full feature list, CLI reference, file locations
    └── aconex/                 Aconex API client library (early stub)
        └── README.md           current state + planned surface
```

A *workspace* is Cargo's equivalent of a .NET solution (`.sln`): it groups
several crates so they share one `target/` build directory and one
`Cargo.lock`. Each crate underneath is independently buildable and, in the
case of `aconex`, independently publishable later.

## The two crates

| Crate | Kind | What it is |
|---|---|---|
| [`paperclip`](crates/paperclip-cli/README.md) | binary | The CLI that discovers PDFs, matches them to binders via a mapper CSV, merges them, and embeds a self-describing manifest. This is the mature part of the project. |
| [`aconex`](crates/aconex/README.md) | library | A from-scratch Rust client for the Aconex API. Currently a stub; will grow to cover authentication, document search, upload, and mail. |

The CLI does not yet depend on the library — the dependency is wired but
commented out until `aconex` has something to call. They are developed together
in this workspace; the library can be split into its own published crate later
without disruption.

## Building

All commands run from the workspace root.

```powershell
cargo build                          # build every crate (debug)
cargo build --release                # optimised build; CLI lands in target\release\
cargo run -p paperclip -- --help     # run the CLI (-p selects the crate)
cargo test -p aconex                 # test a single crate
cargo test                           # test everything
```

The `-p <crate>` (package) flag selects which member a command targets — needed
in a workspace because most commands would otherwise be ambiguous.

## Installing the CLI

After a release build, from the workspace root:

```powershell
.\install.ps1
```

Copies `paperclip.exe` to a per-user location and adds it to the user `PATH`
(no administrator rights required). See the
[CLI README](crates/paperclip-cli/README.md) for details.

## Status at a glance

- **`paperclip` CLI** — functional: binder assembly, classification, embedded
  XMP manifest, inspect/rename detection, skip-and-flag logging.
- **`aconex` library** — scaffolding only. Authentication (username/password
  first, OAuth stubbed for later) is the next slice of work.

See each crate's README for the detailed progress tracker.
