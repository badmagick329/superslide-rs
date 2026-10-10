# Vertical slices over clean architecture

Status: Accepted (2026-10-10).

## Context

By SSR-10 the crate had five features (browsing, loading, viewing, notices, preferences) and three kinds of I/O (filesystem, GPU, UI) in twelve flat modules. Layers had started to leak. Preferences imported the renderer for the magnification filter. `app` and `keymap` imported each other. The scan reached into the decoder for the extension list. `library` and `ui` each mixed pure rules with I/O. The owner's standing rule is that a project past two features plus external I/O is structured as vertical slices over clean architecture.

## Decision

- One folder per feature under `src/`, each holding only the layers it uses: `domain`, `application`, `infrastructure`, `presentation`. [architecture.md](../context/architecture.md) *Shape* lists the slices.
- Dependencies point inward. `shell` (event loop, commands, keymap, wgpu) is the outer layer and the only one that knows every slice. Within a slice, `infrastructure` and `presentation` use `domain`. Across slices, only `domain` is shared, and no slice imports `shell`.
- `image_file` is the shared kernel. `Format` and `Orientation` are needed by browsing, loading and viewing alike, so none of them owns these types.
- `domain` takes no I/O crates. `serde` derives are allowed there, because preferences persist domain types directly.
- A layer is created when code needs it, not up front. No slice has `application` yet: each use case is either a single call into `infrastructure` or lives in `shell::app`. When a use case coordinates several slices or needs testing without its I/O, it becomes the slice's `application`, with a trait standing in for the I/O only if a test needs one.
- Module paths name the layer (`crate::browse::domain::ImageList`), and there are no re-exports, so a wrong-way import shows in the `use` line.

## Consequences

- A feature's issue touches one folder plus `shell`. The preload cache (SSR-11) goes in `load`.
- A test of rules uses only `domain`. Tests that read files sit in `infrastructure`.
- `prefs::domain` depends on other slices' domains, because it aggregates their settings. That is allowed, since it points at a domain, not outward.
- Moving to this layout changed no behaviour. The 43 existing tests moved with their code, and four new ones cover the pieces the split exposed: fitting, one-line damage messages, broken orientation tags and notice expiry.
