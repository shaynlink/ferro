# Ferro local storage

Ferro embeds LiliaDB v0.2 directly in Rust. There is no database daemon, Node runtime, webview,
or cross-platform UI layer involved.

## Layout

- The platform host places the global catalogue in its normal application-data location. It records
  recently opened workspaces as LiliaDB JSON documents.
- A workspace database belongs under `<workspace>/.ferro/ferro.lilia`. It is portable with the
  workspace and is opened through `ferro-storage`.
- Legacy SQLite files are left untouched. Ferro does not read, remove, or migrate them.

## Data models

The current storage adapter persists an indexed text artifact as one atomic LiliaDB batch:

1. a JSON document holds its path and content;
2. a full-text document makes the content searchable;
3. a graph node links dependency edges back to the JSON document.

Workspace activity is stored as a time series. LiliaDB history and 256 undoable commits are enabled
by default. Fast and full integrity checks are available through the `WorkspaceMaintenance` port
and `ferro storage <path>` command.

## Operational rules

Back up the complete LiliaDB directory while Ferro is closed. Do not place a live database on a
network filesystem. Future semantic search will require an explicit, checksum-verified local model
component; source content stays local and the model is never downloaded silently.
