; Inno Setup script for the Windows installer.
; Built by .github/workflows/release.yml:  iscc /DAppVersion=1.2.3 packaging\windows\mvb.iss
; The application name is a placeholder until the final name is chosen.

#define AppName "MVB"
#define AppExe "mvb.exe"
#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif

[Setup]
; Never change AppId: it identifies the application across versions (upgrades, uninstall).
AppId={{6E1C3B57-2B8A-4C7F-9E21-5D4A8F0B73C2}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher=Alexis Meyer
AppPublisherURL=https://github.com/Xiiros/minecraft-view-builder
AppSupportURL=https://github.com/Xiiros/minecraft-view-builder/issues
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
; Installs for the current user without administrator rights, or for everyone if chosen.
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=..\..\dist
OutputBaseFilename=mvb-{#AppVersion}-windows-x64-setup
SetupIconFile=..\icons\mvb.ico
UninstallDisplayIcon={app}\mvb.ico
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ChangesAssociations=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "french"; MessagesFile: "compiler:Languages\French.isl"

[CustomMessages]
english.FileTypes=File types:
french.FileTypes=Types de fichiers :
english.AssociateProject=Open .mvb projects with {#AppName}
french.AssociateProject=Ouvrir les projets .mvb avec {#AppName}
english.AssociateOthers=Add {#AppName} to "Open with" for .litematic, .schem and .nbt files
french.AssociateOthers=Ajouter {#AppName} à « Ouvrir avec » pour les fichiers .litematic, .schem et .nbt
english.ProjectFile={#AppName} project
french.ProjectFile=Projet {#AppName}
english.BuildFile=Block build
french.BuildFile=Construction en blocs

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "assocproject"; Description: "{cm:AssociateProject}"; GroupDescription: "{cm:FileTypes}"
Name: "assocothers"; Description: "{cm:AssociateOthers}"; GroupDescription: "{cm:FileTypes}"

[Files]
Source: "..\..\target\release\{#AppExe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\icons\mvb.ico"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion

[Icons]
Name: "{group}\{#AppName}"; Filename: "{app}\{#AppExe}"; IconFilename: "{app}\mvb.ico"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; IconFilename: "{app}\mvb.ico"; Tasks: desktopicon

[Registry]
; .mvb is our own format: it opens with the application by default.
Root: HKA; Subkey: "Software\Classes\.mvb"; ValueType: string; ValueName: ""; ValueData: "MVB.Project"; Flags: uninsdeletevalue; Tasks: assocproject
Root: HKA; Subkey: "Software\Classes\MVB.Project"; ValueType: string; ValueName: ""; ValueData: "{cm:ProjectFile}"; Flags: uninsdeletekey; Tasks: assocproject
Root: HKA; Subkey: "Software\Classes\MVB.Project\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\mvb.ico"; Tasks: assocproject
Root: HKA; Subkey: "Software\Classes\MVB.Project\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#AppExe}"" ""%1"""; Tasks: assocproject
; Other tools' formats: only offered in "Open with", their default application is not changed.
Root: HKA; Subkey: "Software\Classes\MVB.Build"; ValueType: string; ValueName: ""; ValueData: "{cm:BuildFile}"; Flags: uninsdeletekey; Tasks: assocothers
Root: HKA; Subkey: "Software\Classes\MVB.Build\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\mvb.ico"; Tasks: assocothers
Root: HKA; Subkey: "Software\Classes\MVB.Build\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#AppExe}"" ""%1"""; Tasks: assocothers
Root: HKA; Subkey: "Software\Classes\.litematic\OpenWithProgids"; ValueType: string; ValueName: "MVB.Build"; ValueData: ""; Flags: uninsdeletevalue; Tasks: assocothers
Root: HKA; Subkey: "Software\Classes\.schem\OpenWithProgids"; ValueType: string; ValueName: "MVB.Build"; ValueData: ""; Flags: uninsdeletevalue; Tasks: assocothers
Root: HKA; Subkey: "Software\Classes\.nbt\OpenWithProgids"; ValueType: string; ValueName: "MVB.Build"; ValueData: ""; Flags: uninsdeletevalue; Tasks: assocothers

[Run]
Filename: "{app}\{#AppExe}"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: nowait postinstall skipifsilent
