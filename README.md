# warp-ish

**English** · [Русский](README.ru.md) · [简体中文](README.zh-CN.md)

warp-ish is a local terminal for Windows 11 x64, based on [Warp](https://github.com/warpdotdev/warp). It works offline and opens directly to a shell without requiring a Warp account.

## Install and run

1. Download the Windows x64 setup EXE from [GitHub Releases](https://github.com/thebtf/warp-ish/releases) and verify its SHA-256 against the release notes.
2. Run Setup. It installs for the current Windows user in `%LOCALAPPDATA%\Programs\warp-ish` by default.
3. Open **warp-ish** from the Start menu.

The same release offers a portable ZIP. Extract it into a writable folder, keep the entire `app` folder together, and run `app/warp-ish.exe`.

## Bring your workspace from Warp

Open **Settings → Features → General → Import from Warp**. Import opens a new warp-ish window and profile with compatible settings, saved windows, groups, tabs, splits, and working directories from Warp Stable. It also brings over launch configurations, PowerShell command history, and saved output blocks from completed local commands. After the windows restore successfully, the next ordinary launch uses the imported profile.

The imported panes start fresh shells. Press Up at a prompt to find your commands in **HISTORY** or run your CLI's resume command to continue an existing session. Repeating the import creates another profile. [Import and profile recovery guide](docs/warp-ish/WORKSPACE_IMPORT.md)

## What you can do

- Display images directly in terminal output with Kitty graphics, iTerm2 inline images (OSC 1337), and SIXEL. For example, run `chafa -f sixels photo.png` with Chafa installed. Kitty accepts image data from the program's output stream. This feature ships in `0.2.0`, based on Warp `master @ 1c7fda1191`.
- Use PowerShell in windows, tabs, and split panes with command and output blocks, multiline input, local completion, search, and themes.
- Restore your terminal layout and working directories. Restored panes start fresh shells and wait for your commands.
- Run OMP, SSH, Git, and other command-line tools. Resizing a pane with OMP running directly as `omp` or `omp.exe` avoids the repeated screen switching that caused flicker.
- Let a local agent locate its window, tab group, tab, and pane, open a split or tab, start a peer, and send keys to a specific pane. Enable **Settings → Scripting → Local MCP control**, then open a new pane and configure the client to launch the bundled `warp-ish-mcp.exe` over stdio. It is next to `warp-ish.exe`. [Local MCP guide](docs/warp-ish/LOCAL_MCP.md)

warp-ish leaves out Warp’s built-in AI, account, cloud sync, Drive, sharing, and telemetry. It stores its terminal data separately from Warp.

## Update, reinstall, or roll back

- To update or reinstall, close warp-ish and run the desired version’s Setup under the same Windows user. If you use the ZIP, extract the new package into a separate folder. Settings, history, and layout are stored outside the installation folder and survive uninstall.
- Before updating, back up your profile with warp-ish closed. For the default profile, copy the paths below if present and export the `HKCU\Software\Warp.dev\WarpIsh` registry key if it exists. If you use an imported profile, include its data and the active profile selector. [Profile paths](docs/warp-ish/WORKSPACE_IMPORT.md)
- To roll back, close the new version, save a separate copy of its data, restore the pre-update backup, and launch the previous version. Check the release notes for version-specific migration or recovery steps.

- `%USERPROFILE%\.warp-ish`
- `%APPDATA%\warpish\WarpIsh`
- `%LOCALAPPDATA%\warpish\WarpIsh`

## Source and licenses

warp-ish is an independent fork of [warpdotdev/warp](https://github.com/warpdotdev/warp). See the release notes for the exact upstream and fork commits, corresponding source, package checksum, and checks completed for that release.

The application source uses [AGPL-3.0](LICENSE-AGPL). The Warp UI libraries use [MIT](LICENSE-MIT). The package includes third-party notices in `resources/THIRD_PARTY_LICENSES.txt` next to the executable. See the [original Warp README](README.upstream.md), [upstream provenance](docs/warp-ish/UPSTREAM.md), and [acceptance criteria](docs/warp-ish/ACCEPTANCE.md).
