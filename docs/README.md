# Document index

These documents live under `docs/` and are tracked in Git: commit document changes with the work they describe, and keep credentials and unneeded personal data out of them. Read only what the task needs. Current user decisions, code, configuration and maintained public docs outrank these documents: when one disagrees with a document, correct the document; when two of them disagree with each other, ask the user.

## Project in one paragraph

SuperSlide-RS is a Rust rebuild of Ryan Geiss's SuperSlide (2008–2009), a small, fast, keyboard-driven image viewer for Windows, built for the owner's own use and published on GitHub under the MIT license. Core flow: open an image or folder (drag-and-drop, command line, O / Shift+O), page through the image list with Space and Backspace, zoom and pan, sort or shuffle, run a slideshow, and delete, move or copy files. It ships as one portable Windows executable, attached to each GitHub release; the root `README.md` is the public page. Status: v1 built and verified (SSR-20); until an image list opens, a welcome screen shows the app icon and how to open images; the exe, title bar and taskbar carry the icon; the view zooms, pans, rotates, goes fullscreen and switches to sharp pixels with Ctrl+F, over mipmapped images; egui notices draw over it. Command-line paths, drops, O and Shift+O open an image list; JPEG, PNG, TGA and BMP decode on worker threads into a preload cache, and Space / Backspace or clicks page through it with no blank frames, held keys paced by the advance limit. Sort keys, grouping, per-image screen rotation and the slideshow work, and Delete, M and C send the image to the Recycle Bin, move it or copy it, never overwriting; Ctrl+C copies the image as shown and Ctrl+V shows the clipboard image; ? lists every key, filterable with /; I shows the file name or full details of the image on screen; P edits the preferences, saved beside the executable; the next start reopens the last image list at the same image, in the same order, with the window where it was.

## Working rules for agents

- Record project facts, decisions and the owner's preferences in this collection rather than in agent memory, so the owner can read and edit them.
- The original's behaviour is reference, not spec: [the v1 scope decision](decisions/2026-10-10-v1-scope.md) decides what gets built. When a feature is in neither, ask before building it.

## Where to look

| Location | Purpose and retention |
| --- | --- |
| [context/glossary.md](context/glossary.md) | The ubiquitous language. Update when a concept is added, renamed or retired. |
| [context/architecture.md](context/architecture.md) | Slices and layers, threads, preload cache, rendering, view transform, persistence, preference defaults and Cargo profiles. Rewrite a section when the code it describes changes. |
| [context/keymap.md](context/keymap.md) | Every key and mouse binding; the `keymap` module implements it, and a test checks its table against `keymap::BINDINGS`. Update with any binding change. |
| [context/releasing.md](context/releasing.md) | Cutting a release: tag, workflow, how commit messages become release notes. Update when the release workflow or `cliff.toml` changes. |
| [decisions/2026-10-10-v1-scope.md](decisions/2026-10-10-v1-scope.md) | What v1 keeps and cuts from the original, and why. Supersede with a new ADR when scope changes. |
| [decisions/2026-10-10-rendering-stack.md](decisions/2026-10-10-rendering-stack.md) | Window, GPU, decoding and UI crates, and why the GPU stays without transitions. Supersede when a core crate is replaced. |
| [decisions/2026-10-10-vertical-slices.md](decisions/2026-10-10-vertical-slices.md) | Why the code is vertical slices with inward-pointing layers, and when a slice gains a layer. Supersede if the layout changes. |
| [research/2026-10-10-original-superslide.md](research/2026-10-10-original-superslide.md) | The original's features, key bindings and preferences, from its website. Keep: the page is the only record of an unmaintained app. |
| YouTrack project `SSR` ("SuperSlide-RS") | Issues, bugs and feature requests, read and updated through the `youtrack` MCP tools; open work is `project: SSR #Unresolved`. |

## Language

[context/glossary.md](context/glossary.md) is the project's ubiquitous language. Name things with its terms in conversation, code, UI copy, docs and commits. A new concept gets its row in the same change that introduces it. When two names compete or a term stops fitting, ask the user, then rename everywhere or move the old name under *Avoid*.

## Where new documents go

Before writing, search the index and documents for the topic; extend the owning document when one exists. Load the `writing-for-agents` skill before creating or editing any document here.

- `context/`: current architecture, conventions, operations and gotchas. One home per topic; point to code and configuration rather than copying them.
- `decisions/YYYY-MM-DD-topic.md`: ADRs with status (`Proposed`, `Accepted`, `Superseded`), context, decision and consequences. They record rationale, not live state. Mark a superseded ADR and link its replacement.
- `plans/active/`: substantial unfinished work, with date, status, scope and acceptance criteria. On completion, move lasting rationale to `decisions/` and current guidance to `context/`, then delete the plan, or move it to `plans/completed/` while its evidence still matters.
- `research/` and `audits/`: dated findings with sources and status. An audit finding closes only on execution evidence.
- `artifacts/<task>/`: temporary outputs and helpers. Delete them once the task, and any deployment, is verified.

When this index names a YouTrack project, a bug, idea or backlog item the user decides to defer becomes an issue there, not a document. Raise findings, proposals and questions in chat as usual; open an issue only for work the user has chosen to defer.

Create a folder only when it receives a document. This index is the only file at the docs root.

## Keeping it current

A change is finished only when these documents describe it. Before reporting or committing:

1. **Find.** Search this collection for the names, paths, commands and terms the change touched. Documents you never opened go stale too. Done when every hit is updated or confirmed still true.
2. **Replace.** Rewrite changed guidance where it lives, following `writing-for-agents`, so each section reads as current truth on its own.
3. **One home.** Each fact lives in the document the index names for its topic; everywhere else links to it. Merge any copies the change created or exposed.
4. **Lifecycle.** Plans show actual progress, and finished plans leave `plans/active/`. Delete consumed handovers, status snapshots and verified artifacts once their unique facts have moved. Ask before deleting user-authored or uncertain material.
5. **Language.** Every concept the change introduced or renamed has its glossary row, and no avoided name came back.
6. **Index.** Rows and inbound links match every added, moved or deleted path.
7. **Issues.** When this index names a YouTrack project, every issue the change resolved or advanced gets a comment saying what changed and in which commit; resolve an issue once its change is verified.
