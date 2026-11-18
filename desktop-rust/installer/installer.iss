; CloudStream Desktop — Inno Setup 6 script.
;
; Build with:
;   ISCC.exe installer\installer.iss /DAppVersion=0.1.0
; or let scripts\build.ps1 do it (it also stages dist\).
;
; Payload expectations (produced by scripts\build.ps1):
;   dist\cloudstream-desktop.exe     required
;   dist\libmpv-2.dll                optional, see the note at the bottom
;   dist\LICENSE                     optional
;   dist\NOTICE.txt                  optional

#ifndef AppVersion
  #define AppVersion "0.1.0"
#endif

#define AppName        "CloudStream Desktop"
#define AppPublisher   "CloudStream contributors"
#define AppURL         "https://github.com/recloudstream/cloudstream"
#define AppExeName     "cloudstream-desktop.exe"
; Never change this GUID: it is the identity Windows uses for upgrades.
#define AppId          "{{9F2C4A17-6E3B-4D58-9A55-2C7E1F0B4D63}"

[Setup]
AppId={#AppId}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher={#AppPublisher}
AppPublisherURL={#AppURL}
AppSupportURL={#AppURL}
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
OutputDir=Output
OutputBaseFilename=CloudStreamDesktop-{#AppVersion}-setup
WizardStyle=modern
; LZMA2 solid is the whole reason the payload compresses well; do not change to
; zip or bzip without measuring the resulting installer.
Compression=lzma2/max
SolidCompression=yes
; Windows 10 1607+ only: we rely on per-monitor DPI v2 and modern DPAPI.
MinVersion=10.0
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
; Per-user install by default (no admin prompt), with an all-users option.
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
UninstallDisplayIcon={app}\{#AppExeName}
UninstallDisplayName={#AppName}
VersionInfoVersion={#AppVersion}
VersionInfoDescription={#AppName} installer
VersionInfoProductName={#AppName}
VersionInfoProductVersion={#AppVersion}

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "..\dist\cloudstream-desktop.exe"; DestDir: "{app}"; Flags: ignoreversion
; Optional payload. `skipifsourcedoesntexist` keeps the build working when the
; user has not dropped a libmpv DLL into dist\ yet.
Source: "..\dist\libmpv-2.dll"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
Source: "..\dist\LICENSE"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
Source: "..\dist\NOTICE.txt"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist

[Icons]
Name: "{group}\{#AppName}"; Filename: "{app}\{#AppExeName}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExeName}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#AppExeName}"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
; Only the cache is removed automatically. Bookmarks, resume points and the
; extension registry under %LOCALAPPDATA% are the user's data and survive an
; uninstall; the README explains where they live.
Type: filesandordirs; Name: "{localappdata}\CloudStreamDesktop\cache"

[Code]
procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
  begin
    if not FileExists(ExpandConstant('{app}\libmpv-2.dll')) then
      MsgBox('Video playback needs libmpv.' + #13#10 + #13#10 +
             'The application installed correctly and will run, but it can only ' +
             'show the timeline until libmpv-2.dll is placed next to ' +
             'cloudstream-desktop.exe.' + #13#10 + #13#10 +
             'See docs\ARCHITECTURE.md, section "Size budget".',
             mbInformation, MB_OK);
  end;
end;
