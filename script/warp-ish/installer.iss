#ifndef PayloadDir
  #error PayloadDir must name the verified application payload directory
#endif
#ifndef AppVersion
  #error AppVersion must be supplied by the installer build
#endif
#ifndef OutputDir
  #error OutputDir must be supplied by the installer build
#endif

[Setup]
AppId=dev.warpish.WarpIsh
AppName=warp-ish
AppVersion={#AppVersion}
DefaultDirName={localappdata}\Programs\warp-ish
DisableDirPage=auto
DefaultGroupName=warp-ish
DisableProgramGroupPage=auto
AllowNoIcons=yes
UninstallDisplayIcon={app}\warp-ish.exe
PrivilegesRequired=lowest
MinVersion=10.0.22000
ArchitecturesAllowed=x64os
ArchitecturesInstallIn64BitMode=x64os
CloseApplications=no
RestartApplications=no
OutputDir={#OutputDir}
OutputBaseFilename=warp-ish-{#AppVersion}-windows-x64-setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern

[Files]
Source: "{#PayloadDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{group}\warp-ish"; Filename: "{app}\warp-ish.exe"; WorkingDir: "{app}"
