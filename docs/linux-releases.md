# Linux releases

The fork builds Linux packages when a GitHub release or prerelease is published.
Create the release in `adinschmidt/Handy` using a tag that points to a commit
containing `.github/workflows/linux-release.yml` and the changes to ship.
Select the fork's development branch as the target when creating a new tag.

The **Linux release binaries** workflow attaches AppImage, DEB, and RPM packages
for x86_64 and ARM64 to that release. Packages become available after the build
jobs finish. Saving a draft does not start a build. Failed jobs can be rerun from
the release's workflow run in GitHub Actions.

Before releasing, update the version in `src-tauri/tauri.conf.json`,
`src-tauri/Cargo.toml`, and `package.json` together and refresh `Cargo.lock`.
Use a distinct version for each fork release so installed packages can upgrade.

Builds use the tagged commit and a fresh Rust build. They reuse the repository's
Linux dependency setup and AppImage library checks. Only the built-in
`GITHUB_TOKEN` is required. Packages do not include signed Tauri updater artifacts;
install updates by downloading the new package. The upstream manual **Release**
workflow is separate and requires upstream platform-signing credentials.
