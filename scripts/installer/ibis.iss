#define AppName "ibis"
#define AppExeName "ibis.exe"
#define AppPublisher "ibis"
#ifndef AppVersion
#define AppVersion "0.1.0"
#endif
#ifndef AppSourceDir
#define AppSourceDir "..\..\target\release"
#endif
#ifndef AppOutputDir
#define AppOutputDir "..\..\target\release"
#endif
#ifndef AppOutputBaseFilename
#define AppOutputBaseFilename "ibis-" + AppVersion + "-Setup"
#endif

[Setup]
AppId={{C6A40D2B-8F1B-4B69-AB46-580BBEB680CD}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher={#AppPublisher}
AppVerName={#AppName} {#AppVersion}
DefaultDirName={commonpf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
AllowNoIcons=yes
OutputDir={#AppOutputDir}
OutputBaseFilename={#AppOutputBaseFilename}
SetupIconFile=..\..\ui\assets\appicon.ico
UninstallDisplayIcon={app}\appicon.ico
Compression=lzma2
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=admin
UsedUserAreasWarning=no
WizardStyle=modern

[Tasks]
Name: "startmenuicon"; Description: "Create a Start Menu shortcut"; GroupDescription: "Shortcuts:"; Flags: checkedonce
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "Shortcuts:"; Flags: unchecked

[Files]
Source: "{#AppSourceDir}\{#AppExeName}"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#AppSourceDir}\*.dll"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#AppSourceDir}\appicon.ico"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#AppSourceDir}\README.txt"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#AppSourceDir}\licenses\*"; DestDir: "{app}\licenses"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{group}\{#AppName}"; Filename: "{app}\{#AppExeName}"; WorkingDir: "{app}"; IconFilename: "{app}\appicon.ico"; Tasks: startmenuicon
Name: "{group}\Uninstall {#AppName}"; Filename: "{uninstallexe}"; Tasks: startmenuicon
Name: "{commondesktop}\{#AppName}"; Filename: "{app}\{#AppExeName}"; WorkingDir: "{app}"; IconFilename: "{app}\appicon.ico"; Tasks: desktopicon

[Run]
Filename: "{app}\{#AppExeName}"; Description: "{cm:LaunchProgram,{#StringChange(AppName, '&', '&&')}}"; Flags: nowait postinstall skipifsilent

