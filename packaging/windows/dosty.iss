; Values come from Cargo.toml through scripts/package-windows.ps1.
[Setup]
AppId={{40BFDC7D-4487-44A6-93F8-BA0E57A5C669}
AppName=Dosty
AppVersion={#DostyVersion}
AppPublisher=Moonlark Studios
AppPublisherURL=https://github.com/MoonlarkStudios/dosty
DefaultDirName={localappdata}\Programs\Dosty
DefaultGroupName=Dosty
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.17763
OutputDir={#DostyRoot}\target\dist
OutputBaseFilename=Dosty-{#DostyVersion}-windows-x86_64-setup
SetupIconFile={#DostyRoot}\branding\dosty\icons\dosty.ico
UninstallDisplayIcon={app}\dosty.ico
VersionInfoVersion={#DostyWindowsVersion}
VersionInfoProductVersion={#DostyWindowsVersion}
VersionInfoProductTextVersion={#DostyVersion}
LicenseFile={#DostyRoot}\LICENSE
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
CloseApplications=no
RestartApplications=no

[Files]
Source: "{#DostyRoot}\target\x86_64-pc-windows-msvc\release\dosty.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#DostyRoot}\branding\dosty\icons\dosty.ico"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#DostyRoot}\target\windows-licenses\*"; DestDir: "{app}\licenses"; Flags: ignoreversion

[Icons]
Name: "{group}\Dosty"; Filename: "{app}\dosty.exe"; IconFilename: "{app}\dosty.ico"
