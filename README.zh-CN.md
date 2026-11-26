# warp-ish

[English](README.md) · [Русский](README.ru.md) · **简体中文**

warp-ish 是一款基于 [Warp](https://github.com/warpdotdev/warp) 的本地终端，适用于 Windows 11 x64。它可以离线运行，打开后直接进入 Shell，无需 Warp 账号。

## 安装和运行

1. 从 [GitHub 发布页面](https://github.com/thebtf/warp-ish/releases) 下载 Windows x64 安装程序，并对照发布说明核对 SHA-256。
2. 运行安装程序。它会为当前 Windows 用户安装 warp-ish，默认路径为 `%LOCALAPPDATA%\Programs\warp-ish`。
3. 从「开始」菜单打开 **warp-ish**。

同一版本也提供免安装 ZIP。将整个压缩包解压到有写入权限的文件夹中，保留 `app` 文件夹中的所有文件，然后运行 `app/warp-ish.exe`。

## 从 Warp 导入工作区

打开 **Settings → Features → General → Import from Warp**。导入会打开新的 warp-ish 窗口并创建新的数据配置，带入 Warp Stable 的兼容设置、已保存的窗口、分组、标签页、分屏窗格和工作目录。启动配置、PowerShell 命令历史记录以及已完成本地命令的输出块也会一并导入。窗口成功恢复后，下次正常启动时会使用导入后的配置。

导入的窗格会启动新的 Shell。在命令提示符处按向上键可以打开 **HISTORY**。如需继续已有的命令行会话，请运行相应工具的恢复命令。再次导入会另建一个配置。 [导入及恢复原配置指南](docs/warp-ish/WORKSPACE_IMPORT.md)

## 主要功能

- 通过 Kitty Graphics、iTerm2 内联图像协议（OSC 1337）或 SIXEL 在终端输出中直接查看图像。例如，安装 Chafa 后，可运行 `chafa -f sixels photo.png`。使用 Kitty 协议时，程序通过输出流传输图像数据。此功能自 `0.2.0-rc.1` 起提供，该版本基于 Warp 的 `master @ 1c7fda1191`。
- 在窗口、标签页和分屏窗格中使用 PowerShell。支持命令和输出分块、多行输入、本地补全、搜索和主题。
- 恢复终端布局和各窗格的工作目录。恢复后的窗格会启动新的 Shell，等待你输入命令。
- 运行 OMP、SSH、Git 等命令行工具。直接运行 `omp` 或 `omp.exe` 时，调整窗格大小已不会因反复切换屏幕而闪烁。
- 本地代理可以定位自己所在的窗口、标签页组、标签页和窗格，新建标签页或分屏，启动独立会话，也可以向指定窗格发送按键。启用 **Settings → Scripting → Local MCP control**，打开新窗格，再将客户端配置为通过 stdio 启动 `warp-ish-mcp.exe`。该程序与 `warp-ish.exe` 放在同一目录。 [本地 MCP 指南](docs/warp-ish/LOCAL_MCP.md)

warp-ish 移除了 Warp 的内置 AI、账号、云同步、Drive、共享和遥测功能。warp-ish 的终端数据与 Warp 分开保存。

## 更新、重装和回退

- 更新或重装前，先退出 warp-ish，再以同一个 Windows 用户运行所需版本的安装程序。如果使用 ZIP，请将新版解压到单独的文件夹。设置、历史记录和布局保存在安装目录之外，卸载时会保留。
- 更新前，请在 warp-ish 关闭时备份数据。使用默认配置时，复制下面已存在的目录，并导出注册表项 `HKCU\Software\Warp.dev\WarpIsh`（如有）。使用导入的配置时，还请备份该配置的数据和选定当前配置的文件。 [数据配置路径](docs/warp-ish/WORKSPACE_IMPORT.md)
- 回退前，先退出新版并另存一份新版数据，然后恢复更新前的备份并启动旧版。各版本的迁移和恢复步骤请参阅对应的发布说明。

- `%USERPROFILE%\.warp-ish`
- `%APPDATA%\warpish\WarpIsh`
- `%LOCALAPPDATA%\warpish\WarpIsh`

## 源代码和许可证

warp-ish 是 [warpdotdev/warp](https://github.com/warpdotdev/warp) 的独立分支。各版本的发布说明会标明上游和本项目的 Git 提交、对应源代码、软件包校验值和已完成的检查。

应用源代码采用 [AGPL-3.0](LICENSE-AGPL)，Warp 的界面库采用 [MIT](LICENSE-MIT)。可执行文件所在目录下的 `resources/THIRD_PARTY_LICENSES.txt` 包含第三方许可声明。另请参阅 [Warp 原始说明](README.upstream.md)、[上游来源](docs/warp-ish/UPSTREAM.md)和[验收标准](docs/warp-ish/ACCEPTANCE.md)。
