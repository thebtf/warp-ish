# warp-ish

**English** · [Русский](README.ru.md) · [简体中文](README.zh-CN.md)

warp-ish is a local terminal for Windows 11 x64, based on [Warp](https://github.com/warpdotdev/warp). It works offline and opens directly to a shell without requiring a Warp account.

## What you can do

- Open PowerShell in windows, tabs, and split panes. Use command and output blocks, multiline input, local completion, search, copy and paste, and themes.
- Press Up at the prompt to open **HISTORY**. Filter and select commands, or press Esc to return to your draft. Commands such as `omp --resume …` are stored as ordinary command history.
- Reopen your windows, tabs, and panes with their working directories. Restored panes start fresh shells and wait for you to run commands.
- Run OMP, SSH, Git, and other command-line tools normally. They manage their own sessions, configuration, and network connections.

warp-ish leaves out Warp’s built-in AI, account, cloud sync, Drive, sharing, and telemetry. It stores its terminal data separately from Warp.

## Install and run

1. Download the Windows x64 ZIP from this repository’s Releases page. Check its SHA-256 against the release notes or package receipt.
2. Extract the entire ZIP into a folder you can write to. Keep the contents of the `app` folder together, including its DLLs and resources.
3. Open `app/warp-ish.exe`. You can create a shortcut from the EXE’s context menu in Windows.

## Update or roll back

- To update, quit warp-ish, back up its data, and extract the newer ZIP into a separate folder. Launch the new EXE using the same Windows user and profile. Keep the previous package until you’ve checked your settings, history, and layout.
- To roll back, quit the newer version and keep a separate copy of its data. Restore the pre-update backup that matches the previous version, then launch the previous EXE. Read the release notes for any version-specific migration or recovery steps.

For the default warp-ish profile, back up the following paths if they exist while the application is closed. Include the user’s `HKCU\Software\Warp.dev\WarpIsh` registry key if present.

- `%USERPROFILE%\.warp-ish`
- `%APPDATA%\warpish\WarpIsh`
- `%LOCALAPPDATA%\warpish\WarpIsh`

## Source and licenses

warp-ish is an independent fork of [warpdotdev/warp](https://github.com/warpdotdev/warp). See the release notes for the exact upstream and fork commits, corresponding source, package checksum, and checks completed for that release.

The application source uses [AGPL-3.0](LICENSE-AGPL). The Warp UI libraries use [MIT](LICENSE-MIT). The package includes third-party notices in `app/resources/THIRD_PARTY_LICENSES.txt`. See the [original Warp README](README.upstream.md), [upstream provenance](docs/warp-ish/UPSTREAM.md), and [acceptance criteria](docs/warp-ish/ACCEPTANCE.md).
