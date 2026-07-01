; StormSewer Windows Installer (Inno Setup 6)
; Build: iscc installer\stormsewer.iss
; Code signing (optional): set SIGNTOOL env var, e.g.
;   set SIGNTOOL=signtool sign /fd SHA256 /a /tr http://timestamp.digicert.com /td SHA256

#define MyAppName "StormSewer"
#define MyAppVersion "0.7.0"
#define MyAppPublisher "StormSewer"
#define MyAppExeName "StormSewer.exe"
#define MyAppURL "https://github.com/mf4633/stormsewer"

[Setup]
AppId={{A7B3C4D5-E6F7-4890-ABCD-EF1234567890}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}
DefaultDirName={autopf}\{#MyAppName}
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes
OutputDir=..\dist
OutputBaseFilename=StormSewer-{#MyAppVersion}-setup
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
PrivilegesRequired=lowest
ArchitecturesInstallIn64BitMode=x64

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "..\target\release\{#MyAppExeName}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\examples\demo.ssproj"; DestDir: "{app}\examples"; Flags: ignoreversion
Source: "..\examples\investor-demo.ssproj"; DestDir: "{app}\examples"; Flags: ignoreversion
; Sign release binary before packaging:
;   signtool sign /fd SHA256 /a /tr http://timestamp.digicert.com /td SHA256 target\release\StormSewer.exe

[Icons]
Name: "{group}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"
Name: "{group}\Demo Project"; Filename: "{app}\{#MyAppExeName}"; Parameters: ""; WorkingDir: "{app}\examples"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "{cm:LaunchProgram,{#StringChange(MyAppName, '&', '&&')}}"; Flags: nowait postinstall skipifsilent