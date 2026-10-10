# Releasing

A release is a pushed tag. [release.yml](../../.github/workflows/release.yml) then tests and builds `superslide.exe` on `windows-latest` and publishes a GitHub release holding it, with notes from [cliff.toml](../../cliff.toml).

1. Set `version` in `Cargo.toml` to the new version and commit it (`chore: release 0.0.2`). The workflow fails when the tag and that version differ.
2. `git tag v0.0.2`, then `git push origin main v0.0.2`.

## Release notes

git-cliff reads the commit subjects since the previous tag. Only `feat:` and `fix:` commits appear, under *Features* and *Fixes*; `docs`, `refactor`, `chore` and anything unconventional stay out. Issue keys like `(SSR-15)` are stripped, since readers cannot open the tracker. So a `feat:` or `fix:` subject is user-facing copy: write it for someone who downloads the exe. Preview with `git cliff --unreleased`.

## The exe

Built with `-C target-feature=+crt-static`, so it runs without the Visual C++ redistributable. It is unsigned, so Windows SmartScreen warns on first run.
