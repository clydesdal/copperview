; The Windows installer, built by make-windows-installer.ps1 with Inno Setup 6.
; That script passes in the version and where the program and the output go:
;   ISCC /DVersion=0.2.1 /DExe=..\..\target\release\copperview.exe /DOutDir=..\..\dist copperview.iss
;
; It installs for the current person by default, with no administrator needed,
; and offers to install for everyone instead. Settings > Apps removes it again.

#ifndef Version
  #error Version is needed: ISCC /DVersion=x.y.z
#endif
#ifndef Exe
  #define Exe "..\..\target\release\copperview.exe"
#endif
#ifndef OutDir
  #define OutDir "..\..\dist"
#endif

[Setup]
; Never change this: it is how Windows knows a new version replaces an old one.
AppId={{81AE5E2C-9FEC-41F2-9EAD-40E2DF636778}
AppName=Copperview
AppVersion={#Version}
AppVerName=Copperview {#Version}
AppPublisher=Circuitapps
AppComments=Look at PCB Gerber files, measure anything you click, and compare two sets.
VersionInfoVersion={#Version}
DefaultDirName={autopf}\Copperview
DefaultGroupName=Copperview
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
LicenseFile=..\..\LICENSE
SetupIconFile=copperview.ico
UninstallDisplayIcon={app}\copperview.ico
UninstallDisplayName=Copperview
OutputDir={#OutDir}
OutputBaseFilename=copperview-setup-{#Version}
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ChangesEnvironment=yes
ChangesAssociations=yes
CloseApplications=yes

[Tasks]
Name: "desktopicon"; Description: "Put Copperview on the desktop"; Flags: unchecked
Name: "addtopath"; Description: "Let a command prompt run copperview (adds it to PATH)"
Name: "openwith"; Description: "Add ""Open in Copperview"" when right-clicking a folder or zip"; Flags: unchecked

[Files]
Source: "{#Exe}"; DestDir: "{app}"; DestName: "copperview.exe"; Flags: ignoreversion
Source: "copperview.ico"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion
Source: "..\..\CHANGELOG.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\fonts\OFL.txt"; DestDir: "{app}"; DestName: "Figtree-OFL.txt"; Flags: ignoreversion

[Icons]
; The icon comes from its own file too, in case the build couldn't put it in the program.
Name: "{autoprograms}\Copperview"; Filename: "{app}\copperview.exe"; IconFilename: "{app}\copperview.ico"
Name: "{autodesktop}\Copperview"; Filename: "{app}\copperview.exe"; IconFilename: "{app}\copperview.ico"; Tasks: desktopicon

[Registry]
; Copperview 0.1.0 put itself in Settings > Apps by hand, under this name. This
; installer takes over that place, so the old entry goes.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Uninstall\Copperview"; Flags: deletekey dontcreatekey
; Right-click a folder or a zip: Open in Copperview.
Root: HKA; Subkey: "Software\Classes\Directory\shell\Copperview"; ValueType: string; ValueName: ""; ValueData: "Open in Copperview"; Flags: uninsdeletekey; Tasks: openwith
Root: HKA; Subkey: "Software\Classes\Directory\shell\Copperview"; ValueType: string; ValueName: "Icon"; ValueData: """{app}\copperview.ico"""; Tasks: openwith
Root: HKA; Subkey: "Software\Classes\Directory\shell\Copperview\command"; ValueType: string; ValueName: ""; ValueData: """{app}\copperview.exe"" ""%1"""; Tasks: openwith
Root: HKA; Subkey: "Software\Classes\SystemFileAssociations\.zip\shell\Copperview"; ValueType: string; ValueName: ""; ValueData: "Open in Copperview"; Flags: uninsdeletekey; Tasks: openwith
Root: HKA; Subkey: "Software\Classes\SystemFileAssociations\.zip\shell\Copperview"; ValueType: string; ValueName: "Icon"; ValueData: """{app}\copperview.ico"""; Tasks: openwith
Root: HKA; Subkey: "Software\Classes\SystemFileAssociations\.zip\shell\Copperview\command"; ValueType: string; ValueName: ""; ValueData: """{app}\copperview.exe"" ""%1"""; Tasks: openwith

[Run]
Filename: "{app}\copperview.exe"; Description: "Open Copperview now"; Flags: nowait postinstall skipifsilent

[Code]
// PATH: the person's own when installed just for them, everyone's otherwise.
function PathRoot: Integer;
begin
  if IsAdminInstallMode then Result := HKEY_LOCAL_MACHINE else Result := HKEY_CURRENT_USER;
end;

function PathKey: String;
begin
  if IsAdminInstallMode then
    Result := 'SYSTEM\CurrentControlSet\Control\Session Manager\Environment'
  else
    Result := 'Environment';
end;

function HasEntry(Path, Dir: String): Boolean;
begin
  Result := Pos(';' + Uppercase(Dir) + ';', ';' + Uppercase(Path) + ';') > 0;
end;

procedure AddToPath(Dir: String);
var
  Path: String;
begin
  if not RegQueryStringValue(PathRoot, PathKey, 'Path', Path) then Path := '';
  if HasEntry(Path, Dir) then Exit;
  if (Path <> '') and (Copy(Path, Length(Path), 1) <> ';') then Path := Path + ';';
  RegWriteExpandStringValue(PathRoot, PathKey, 'Path', Path + Dir);
end;

procedure RemoveFromPath(Dir: String);
var
  Path: String;
  At: Integer;
begin
  if not RegQueryStringValue(PathRoot, PathKey, 'Path', Path) then Exit;
  if not HasEntry(Path, Dir) then Exit;
  Path := ';' + Path + ';';
  At := Pos(';' + Uppercase(Dir) + ';', Uppercase(Path));
  Delete(Path, At, Length(Dir) + 1);
  Path := Copy(Path, 2, Length(Path) - 2);
  RegWriteExpandStringValue(PathRoot, PathKey, 'Path', Path);
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if (CurStep = ssPostInstall) and WizardIsTaskSelected('addtopath') then
    AddToPath(ExpandConstant('{app}'));
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usPostUninstall then
    RemoveFromPath(ExpandConstant('{app}'));
end;
