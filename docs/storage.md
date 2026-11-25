# Storage experiment

`ferro-storage` is currently an internal LiliaDB-backed experiment. It validates how an embedded
data service could be offered to future Ferro applications without making a database implementation
part of a target UI or of the core domain layer.

It does **not** define Ferro's product purpose, project model, or public persistence API.

## Current implementation

The crate currently embeds LiliaDB in Rust and exposes prototype adapters for a global catalogue
and a workspace-local database. The reference macOS host uses these adapters while the framework
bootstrap is being developed. The `ferro storage <path>` command can run an integrity check on a
local database path.

This is experimental code, not a compatibility promise for Ferro application developers. Its paths,
schemas, data models, and command shape may change once Ferro has a public project manifest and SDK
service model.

## Framework direction

A future persistence capability must be defined as an explicit SDK service with clear ownership,
lifecycle, migration, backup, and target-support semantics. An application should opt into it;
Ferro must not impose an end-user workspace catalogue or a particular database model on every app.

LiliaDB remains a candidate embedded implementation because it can support local documents,
full-text search, graphs, activity series, integrity checks, and history. Those capabilities become
part of Ferro only when they support a documented framework workflow and a stable public contract.

## Operational guidance for the prototype

Back up a complete LiliaDB directory only while its owning process is closed. Do not place a live
database on a network filesystem. There is no automatic import, migration, or removal of legacy
SQLite files.
