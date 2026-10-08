; Invoke ISCC directly with PayloadDir, InstallerDir and AppVersion; no helper scripts are called.
#ifndef PayloadDir
  #error PayloadDir must point to a prepared Nanobug distribution
#endif
#ifndef InstallerDir
  #error InstallerDir must name the installer output directory
#endif
#ifndef AppVersion
  #error AppVersion must match Cargo.toml
#endif

[Setup]
; Keep this identity stable so subsequent versions upgrade the same per-user application.
AppId=Nanobug.Editor
AppName=Nanobug
AppVersion={#AppVersion}
AppPublisher=Nanobug contributors
AppPublisherURL=https://github.com/T-miracle/Editor
DefaultDirName={localappdata}\Programs\Nanobug
DefaultGroupName=Nanobug
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir={#InstallerDir}
OutputBaseFilename=Nanobug-Setup-{#AppVersion}-x64
SetupIconFile={#PayloadDir}\Nanobug.ico
UninstallDisplayIcon={app}\Nanobug.exe
UninstallDisplayName=Nanobug
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
DisableProgramGroupPage=yes
; The GUI holds this mutex. Never force-close a document or restart the editor during an upgrade.
AppMutex=Local\Nanobug.Installer.Running
CloseApplications=no
RestartApplications=no
SetupLogging=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "chinesesimplified"; MessagesFile: "languages\ChineseSimplified.isl"

[Tasks]
; Desktop integration is optional; no file associations or PATH changes are applied silently.
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#PayloadDir}\Nanobug.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#PayloadDir}\Nanobug.ico"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#PayloadDir}\plugins\*"; DestDir: "{app}\plugins"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "{#PayloadDir}\licenses\*"; DestDir: "{app}\licenses"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{userprograms}\Nanobug"; Filename: "{app}\Nanobug.exe"; WorkingDir: "{userdocs}"
Name: "{userdesktop}\Nanobug"; Filename: "{app}\Nanobug.exe"; WorkingDir: "{userdocs}"; Tasks: desktopicon

; User settings, installed plugins, history and caches remain outside {app} and survive uninstall.
