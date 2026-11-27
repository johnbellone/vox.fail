# Desktop client builds and releases

The desktop client (`vox-client`) is built for Linux, Windows and macOS by GitHub Actions. This
page says what is built, where the files go, and how to make a release. The workflows are in
[.github/workflows](../../.github/workflows/).

## What is built

[client-build.yml](../../.github/workflows/client-build.yml) builds the client for each system,
packages it, runs a smoke test on the package (`vox-client --check`, and for Windows an
install and uninstall), and uploads the files as workflow artifacts. Each build has five files.
`<v>` is the version:

| File | System |
| --- | --- |
| `Vox-<v>-linux-x86_64.AppImage` | Linux (built on Ubuntu 22.04 so it runs on more distributions) |
| `Vox-<v>-linux-x86_64.tar.gz` | Linux, a folder with the program and its files |
| `Vox-<v>-windows-x64-setup.exe` | Windows installer |
| `Vox-<v>-windows-x64.zip` | Windows, a folder you unzip and run without installing |
| `Vox-<v>-macos-universal.dmg` | macOS, one file for Apple silicon and Intel |

The version is `nightly` for nightly builds, `pr` for a pull request, `dev` for a manual run
(unless you set another), and `x.y.z` for a release.

## Where the files go

- Windows: the installer puts the game in `%LOCALAPPDATA%\Programs\Vox`. It installs for your user
  only and needs no administrator rights. Your own files are in `%APPDATA%\Vox`, and the uninstaller
  keeps that folder.
- macOS: the app is `Vox.app`, and the program's files are in `Vox.app/Contents/Resources`.
- Linux AppImage: the program's files are in `/usr/share/vox` inside the AppImage.
- Your own files (`controls.json` and `levels/`) are kept apart from the program, in a Vox folder for
  you: `%APPDATA%\Vox` on Windows, `~/Library/Application Support/Vox` on macOS, and
  `$XDG_DATA_HOME/vox` (`~/.local/share/vox`) on Linux.

## Nightly

[nightly.yml](../../.github/workflows/nightly.yml) runs every day at 05:17 UTC, and on demand
from the Actions tab.

- It builds only commits on the default branch.
- If the commit is the one the `nightly` tag already points at, it stops. To rebuild it anyway,
  run the workflow by hand and set `force` to `true`.
- Otherwise it builds the five files, uploads them to the `nightly` prerelease, and moves the
  `nightly` tag to that commit. Files from an earlier nightly that the new build does not have
  are removed.

Nightly builds are unsigned test builds. The release notes say so.

## Making a release

1. Pick a version `x.y.z`, or `x.y.z-pre` for a prerelease (for example `1.2.0-rc1`).
2. Tag the commit and push the tag:

   ```bash
   git tag vX.Y.Z
   git push origin vX.Y.Z
   ```

3. [release.yml](../../.github/workflows/release.yml) runs on every `v*` tag. It builds the five
   files, then creates the GitHub release for the tag. The title is `Vox <version>` and the
   notes are generated from the commits. A version with a `-` in it is marked as a prerelease.
   The version must have the form `x.y.z` or `x.y.z-pre`; other tags do not make a client release.
4. Re-running the workflow for the same tag replaces the files on the release.

[windows.yml](../../.github/workflows/windows.yml) runs on the same tags and adds the Block
Avatar files to the same release.

## Testing a change on a pull request

The client build runs on a pull request that changes `bins/vox-client/`, `crates/vox-world/`,
`crates/vox-core/` or `client-build.yml`. Its five files are on the workflow run as artifacts,
with the version `pr`. You can also run the build by hand from the Actions tab (Vox client build,
Run workflow), with a version of your choice.

## Signing

The builds are not signed with a developer certificate unless the repository has these secrets. A
signing step runs when its certificate is set, and it needs the matching password. Without the macOS
certificate, the macOS app is ad-hoc signed.

| Secret | What it holds |
| --- | --- |
| `WINDOWS_CERT_PFX` | The Windows code-signing certificate (`.pfx`), base64-encoded |
| `WINDOWS_CERT_PASSWORD` | The password of the `.pfx` |
| `MACOS_CERT_P12` | The Developer ID Application certificate (`.p12`), base64-encoded |
| `MACOS_CERT_PASSWORD` | The password of the `.p12` |
| `APPLE_ID` | The Apple ID used to notarize the app |
| `APPLE_TEAM_ID` | The Apple developer team ID |
| `APPLE_APP_PASSWORD` | An app-specific password for that Apple ID |

The Windows program and installer are signed when the certificate is set. The macOS app is
notarized only when the certificate and all three Apple secrets are set.

## Checking a build

Every build of the client accepts `--check`. It prints the version, the folder with the game's
files, and the player's own folder, then exits with 0 if the game's files are all there. If one
is missing, it names it and exits with 1. `--version` prints only the version.

```bash
vox-client --check
```
