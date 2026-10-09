#define MyAppName "Simple MKV Player"
#define MyAppPublisher "Simple MKV Player"
#define MyAppExeName "Simple MKV Player.exe"

#ifndef SourceDir
  #error SourceDir must be supplied by scripts\build-release.ps1
#endif

#ifndef AppVersion
  #define AppVersion "0.1.0"
#endif

#ifndef OutputDir
  #define OutputDir "."
#endif

[Setup]
AppId={{3A8A0A51-2D84-4E94-B3B0-33CDB3FB2257}
AppName={#MyAppName}
AppVersion={#AppVersion}
AppPublisher={#MyAppPublisher}
AppCopyright=Copyright (c) 2026 Simple MKV Player contributors
VersionInfoVersion={#AppVersion}.0
VersionInfoProductName={#MyAppName}
VersionInfoProductVersion={#AppVersion}
VersionInfoDescription=Multi-track MKV video player and audio mixer
DefaultDirName={autopf}\{#MyAppName}
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes
PrivilegesRequired=admin
OutputDir={#OutputDir}
OutputBaseFilename=Simple_MKV_Player_v{#AppVersion}_Setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
SetupIconFile=..\src\assets\smkv_logo.ico
UninstallDisplayIcon={app}\smkv_logo.ico
UninstallDisplayName={#MyAppName}
CloseApplications=yes
RestartApplications=no

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Additional shortcuts:"; Flags: unchecked

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autoprograms}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; WorkingDir: "{app}"; IconFilename: "{app}\smkv_logo.ico"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; WorkingDir: "{app}"; IconFilename: "{app}\smkv_logo.ico"; Tasks: desktopicon

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "Launch {#MyAppName}"; Flags: nowait postinstall skipifsilent
