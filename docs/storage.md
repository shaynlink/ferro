# Storage and optional developer services

`ferro-storage` embeds LiliaDB and exposes optional developer services without making a database
implementation part of a target UI or of the core domain layer.

It does **not** define Ferro's product purpose or project model.

## Current implementation

The public optional surface is Preferences and Recent Resources, backed exclusively by LiliaDB.
The crate also retains prototype adapters for a global catalogue and workspace-local database. The
`ferro storage <path>` command can run an integrity check on a local database path.

The public service contracts are the compatibility boundary. Prototype catalogue and indexing paths,
schemas and data models remain subject to change.

## Framework direction

Persistence has clear ownership through explicit SDK services. An application opts into it; Ferro
does not impose an end-user workspace catalogue or database dependency on every app.

LiliaDB is the sole supported embedded implementation. SQLite is not a Ferro backend, feature or
migration target. Documents, search, graphs, activity series, integrity checks and history remain
internal until they support a documented framework workflow and stable public contract.

## Operational guidance for the prototype

Back up a complete LiliaDB directory only while its owning process is closed. Do not place a live
database on a network filesystem.
