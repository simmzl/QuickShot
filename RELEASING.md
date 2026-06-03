# Releasing QuickShot

Releases are built and published by the **Release** GitHub Actions workflow
(`.github/workflows/release.yml`). It triggers **only on tags matching `v*`** —
pushing commits to `master` does **not** build a release.

## Why the version must be bumped first

The packaging scripts derive the artifact filenames **and** the macOS
`Info.plist` version from the `version` field in `Cargo.toml`, **not** from the
git tag:

- `scripts/package.sh`  → `dist/QuickShot-<version>.dmg`
- `scripts/package.ps1` → `dist/QuickShot-<version>-windows-x64.zip`

The release **title** uses the tag (`QuickShot ${{ github.ref_name }}`), but the
**files inside it** use `Cargo.toml`'s version. So if you bump the tag without
bumping `Cargo.toml`, the artifacts keep the old version in their names. Keep the
two in lockstep.

## Steps for every release

1. **Bump the version** in `Cargo.toml` to the target version, e.g.:
   ```toml
   [package]
   version = "1.1.6"
   ```
2. **Sync `Cargo.lock`** so it records the same version:
   ```bash
   cargo check
   ```
3. **Commit** both files:
   ```bash
   git add Cargo.toml Cargo.lock
   git commit -m "chore(release): bump version to 1.1.6"
   git push origin master
   ```
4. **Tag and push** — the tag name must start with `v` and match the version:
   ```bash
   git tag v1.1.6
   git push origin v1.1.6
   ```
5. The **Release** workflow runs automatically:
   - `build-macos` — universal `.app` + `.dmg`
   - `build-windows` — MSVC `.exe` + `.zip`
   - `release` — publishes a single GitHub Release with both artifacts

   Both build jobs must pass before `release` runs. If a build fails, fix it on
   `master`, then cut the next patch tag (you can't re-run a tag that already
   published).

## Verify

After the run finishes:

```bash
gh run list --workflow Release --limit 1
gh release view v1.1.6
```

Artifacts should be named `QuickShot-1.1.6.dmg` and
`QuickShot-1.1.6-windows-x64.zip`.

## Notes

- The Windows `.exe` icon is embedded at build time by `build.rs` via the
  `winresource` crate from `assets/app-icon.ico` (Windows host only).
- Icon/branding assets live in `assets/`. The `.svg` files are the design
  sources; the `.png`/`.ico` files are exported from them.
