# Project Notebook

`Project Notebook` is Ferro's end-to-end macOS integration example. It keeps portable state in
`crates/project-notebook-core` and owns native AppKit rendering in `apps/project-notebook-macos`.

Run it from this repository on macOS:

```sh
cargo run -p ferro -- run --project examples/project-notebook
```

Choose a folder with `Cmd+O`; the example creates `.ferro-notebook/data.lilia` below it. The
example is intentionally native: no shared widget tree is involved.
