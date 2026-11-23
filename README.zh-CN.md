# warp-ish

[English](README.md) · [Русский](README.ru.md) · **简体中文**

warp-ish 是一款基于 [Warp](https://github.com/warpdotdev/warp) 的本地终端，适用于 Windows 11 x64。它可以离线运行，打开后直接进入 Shell，无需 Warp 账号。

## 主要功能

- 在窗口、标签页和分屏窗格中使用 PowerShell。支持命令和输出分块、多行输入、本地补全、搜索、复制粘贴和主题。
- 在命令提示符处按向上键打开 **HISTORY**。可以筛选并选择命令，也可以按 Esc 返回输入草稿。`omp --resume …` 等命令会和其他命令一样保存在历史记录中。
- 重新打开先前的窗口、标签页和窗格，并恢复各窗格的工作目录。恢复后的窗格会启动新的 Shell，等待你输入命令。
- 照常运行 OMP、SSH、Git 等命令行工具。这些程序自行管理会话、配置和网络连接。

warp-ish 移除了 Warp 的内置 AI、账号、云同步、Drive、共享和遥测功能。warp-ish 的终端数据与 Warp 分开保存。

## 安装和运行

1. 从本仓库的 Releases 页面下载 Windows x64 ZIP 压缩包。对照发布说明或软件包校验记录，核对 SHA-256。
2. 将整个 ZIP 解压到你有写入权限的文件夹中。`app` 文件夹中的 DLL、资源和其他文件需要保留在一起。
3. 打开 `app/warp-ish.exe`。如需快捷方式，可以在 Windows 中右键单击 EXE 创建。

## 更新和回退

- 更新前，退出 warp-ish 并备份数据，然后将新版 ZIP 解压到单独的文件夹。使用同一个 Windows 用户和数据配置启动新版 EXE。确认设置、历史记录和窗口布局正常之前，请保留旧版软件包。
- 如需回退，先退出新版并另存一份新版数据，再恢复更新前的备份，启动旧版 EXE。各版本的迁移或恢复步骤请参阅对应的发布说明。

使用默认的 warp-ish 配置时，请在应用关闭后备份以下已存在的目录。如果存在注册表项 `HKCU\Software\Warp.dev\WarpIsh`，也请一并备份。

- `%USERPROFILE%\.warp-ish`
- `%APPDATA%\warpish\WarpIsh`
- `%LOCALAPPDATA%\warpish\WarpIsh`

## 源代码和许可证

warp-ish 是 [warpdotdev/warp](https://github.com/warpdotdev/warp) 的独立分支。各版本的发布说明会标明上游和本项目的 Git 提交、对应源代码、软件包校验值和已完成的检查。

应用源代码采用 [AGPL-3.0](LICENSE-AGPL)，Warp 的界面库采用 [MIT](LICENSE-MIT)。压缩包中的 `app/resources/THIRD_PARTY_LICENSES.txt` 包含第三方许可声明。另请参阅 [Warp 原始说明](README.upstream.md)、[上游来源](docs/warp-ish/UPSTREAM.md)和[验收标准](docs/warp-ish/ACCEPTANCE.md)。
