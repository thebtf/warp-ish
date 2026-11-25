# Windows installer

Setup installs warp-ish for the current user on Windows 11 x64. The default destination is `%LOCALAPPDATA%\Programs\warp-ish`. The Start Menu entry launches `warp-ish.exe`. The stdio companion `warp-ish-mcp.exe` is installed beside it.

Uninstall removes the application and its shortcut. The terminal profile, settings and history live outside the installation directory and are preserved.

Build with Python 3.11 or later and the official Inno Setup 6.7.4 compiler. Use the directory containing the verified portable ZIP, its package manifest, and its acceptance receipt.

```powershell
python -B script/warp-ish/build-installer.py --package-dir $package --iscc $iscc --output-dir $output
```

The output directory receives the setup executable and `installer.json`, which records the setup hash, the verified ZIP and manifest hashes, and the source and compiler identities. [Windows acceptance](ACCEPTANCE.md) covers runtime behavior.
