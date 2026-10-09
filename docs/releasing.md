# Releasing wy

Releases use [cargo-dist](https://axodotdev.github.io/cargo-dist/book/quickstart/rust.html) to build binaries and generate a platform-detecting shell installer. Configuration lives in `dist-workspace.toml`; `.github/workflows/release.yml` is generated from it. Use cargo-dist **0.33.0** when updating the configuration, then run `dist generate`.

## First release

The one-command binary installer is not live until a release has been published. The README currently uses a direct Cargo install from GitHub so users do not have to clone the repository.

1. Commit and push the release configuration to `main`.
2. Open **GitHub → Actions → Release → Run workflow**. Leave the tag as **dry-run** first. This runs tests on every target platform and builds all archives and the installer without publishing a release.
3. Check that all four builds pass and inspect the workflow artifacts. Try the binary for your platform with `wy --version` and `wy --help`.
4. Run the workflow again with **v0.2.0**, matching the version in `Cargo.toml`. This publishes the GitHub release, archives, checksums and installer. For later releases, update the Cargo version and lockfile before running it with the new tag.
5. Test the published installer in a temporary directory, then replace the README's Cargo install command with the public installer command below. Keep the Cargo command in the usage guide as a source-build option.

GitHub Actions uses the repository's `GITHUB_TOKEN`; no package registry account or separate publishing token is needed. Pull requests check the release plan. Pushing a tag alone does not publish a release; this workflow uses the **Run workflow** button.

## Public install command

After the first release is available:

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/grandimam/wy/releases/latest/download/wy-code-installer.sh | sh
```

This installs the `wy` executable into `~/.local/bin` and updates supported shell profiles. Users open a new terminal and run `wy` from their repository. They need Git, plus Codex or Claude Code if they want agent explanations. A Rust toolchain is not required for binary installation.

The build matrix covers Apple Silicon and Intel macOS, and ARM64 and x64 Linux. Linux artifacts are built on Ubuntu 22.04 with glibc; they are not Alpine/musl builds. Downloadable archives include the binary, README and license. Each archive has a SHA-256 checksum, and the installer verifies the selected archive before installing it.

## Verify locally

```bash
dist generate --check
dist plan
dist build
```

`dist build` builds for the current machine. CI verifies the other configured platforms. The generated installer uses release download URLs, so a normal installation requires the published artifacts.

To test a published installer without modifying shell profiles:

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/grandimam/wy/releases/latest/download/wy-code-installer.sh -o /tmp/wy-installer.sh
wy_test_dir="$(mktemp -d)"
WY_CODE_UNMANAGED_INSTALL="$wy_test_dir" sh /tmp/wy-installer.sh
"$wy_test_dir/wy" --version
"$wy_test_dir/wy" --help
```
