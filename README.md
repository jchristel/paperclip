# Paperclip

A Rust workspace containing a Windows CLI tool for assembling PDFs into named
binders, plus a client library for the Aconex cloud document platform.

## Workspace layout

```
paperclip/                      workspace root (this file)
├── Cargo.toml                  workspace manifest — groups the member crates
├── install.ps1                 per-user installer for the CLI (no admin needed)
└── crates/
    ├── paperclip-cli/          the binder CLI + Aconex commands → paperclip.exe
    │   └── README.md           full feature list, CLI reference, file locations
    └── aconex/                 Aconex API client library
        └── README.md           library API, design, endpoint coverage
```

A *workspace* is Cargo's equivalent of a .NET solution (`.sln`): it groups
several crates so they share one `target/` build directory and one
`Cargo.lock`. Each crate underneath is independently buildable and, in the
case of `aconex`, independently publishable later.

## The two crates

| Crate | Kind | What it is |
|---|---|---|
| [`paperclip`](crates/paperclip-cli/README.md) | binary | The CLI. Discovers PDFs, matches them to binders via a mapper CSV, merges them, and embeds a self-describing manifest. Also hosts the `aconex` subcommands that drive the library. |
| [`aconex`](crates/aconex/README.md) | library | A from-scratch async Rust client for the Aconex API. Currently covers authentication, project lookup, and document register search; more endpoints planned. |

The CLI depends on the library via a path dependency (`aconex = { path =
"../aconex" }`). They are developed together in this workspace; the library has
no CLI-specific dependencies and can be split into its own published crate later
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
  XMP manifest, inspect/rename detection, skip-and-flag logging. Plus Aconex
  commands: list projects, resolve the configured project, search the register,
  and a `diag` group of low-level API probes.
- **`aconex` library** — async client working end to end: Basic auth (OAuth
  stubbed behind the same trait), typed project lookup, and typed, paginated
  document search. Built on `reqwest` + `tokio` + `quick-xml`.

See each crate's README for the detailed progress tracker.

## A note on the relationship to the original Python

The `aconex` crate is a clean-room reimplementation of an existing Python
library, deliberately restructured (split modules, typed structs, an error
enum, the builder/trait patterns) rather than transliterated. Endpoint URLs,
rate limits, and similar facts about the Aconex API are reproduced because they
are facts about the service, not creative expression.
