; Per-user install. Registers passkeep in Start and in Settings > Apps.
#define AppVersion "0.1.0"

[Setup]
AppId={{7E4C1A2B-6F93-4D58-9A10-3C8B5E2F71D4}
AppName=passkeep
AppVersion={#AppVersion}
AppPublisher=gotylib
AppPublisherURL=https://github.com/gotylib/passkeep
DefaultDirName={localappdata}\Programs\passkeep
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
OutputDir=..\target\release
OutputBaseFilename=passkeep-windows-setup
SetupIconFile=..\target\passkeep.ico
UninstallDisplayIcon={app}\passkeep.exe
UninstallDisplayName=passkeep
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
CloseApplications=yes

[Files]
Source: "..\target\release\passkeep.exe"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\passkeep"; Filename: "{app}\passkeep.exe"; AppUserModelID: "gotylib.passkeep"

[Run]
Filename: "{app}\passkeep.exe"; Description: "Launch passkeep"; Flags: nowait postinstall skipifsilent
