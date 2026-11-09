; Recvrve installer (Inno Setup 6.3 or later).
;
; Build the app first (`cargo build --release`), then from the repository root:
;   iscc installer\recvrve.iss
; or with another version:
;   iscc /DAppVersion=0.2.1 installer\recvrve.iss
; The setup program lands in target\installer\.
;
; What it does:
; - the usual wizard: welcome, license, install folder, Start menu folder, desktop shortcut
;   and file associations, ready, install, finish (with "Launch Recvrve");
; - installs Recvrve.exe, the champion portraits and the shipped config, plus the
;   `recvrve-installed` marker that makes the app keep settings in %APPDATA%\Recvrve and
;   saves and exports in Documents\Recvrve (Program Files can't be written to);
; - installs Microsoft's WebView2 Runtime (which the app's window runs on) if it is
;   missing, downloading Microsoft's small bootstrapper;
; - plays some music while the wizard is open, with a Mute button;
; - an uninstaller in Settings > Apps. Uninstalling leaves the user's saves and settings.

#ifndef AppVersion
  #define AppVersion "0.2.1"
#endif
#define AppName "Recvrve"
#define AppExe "Recvrve.exe"
#define AppTitle AppName + " " + AppVersion + " Beta"

[Setup]
; Keep this id forever: it is how a newer setup finds and upgrades an installed copy.
AppId={{A638155A-DD92-417C-A88A-B0CAB86A03EF}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppTitle}
AppPublisher=odparovacc
AppPublisherURL=https://github.com/odparovacc
AppSupportURL=https://github.com/odparovacc/draftree
VersionInfoVersion={#AppVersion}
VersionInfoProductName={#AppName}
VersionInfoDescription={#AppTitle} Setup
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableWelcomePage=no
DisableDirPage=no
DisableProgramGroupPage=no
LicenseFile=license.txt
OutputDir=..\target\installer
OutputBaseFilename={#AppName}-{#AppVersion}-beta-setup
; The Recvrve logo (assets\icon\recvrve.svg, rendered to the .ico); shortcuts, the
; uninstall entry and associated files take it from the executable.
SetupIconFile=..\assets\icon\recvrve.ico
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName={#AppTitle}
WizardStyle=modern
; The Recvrve logo on the left of the welcome and finish pages, the spoon in the top right corner of
; the other pages (and, see [Code], of the welcome page).
WizardImageFile=assets\logo-164x314.bmp,assets\logo-328x628.bmp
WizardSmallImageFile=assets\spoon-55.bmp,assets\spoon-110.bmp
WizardImageStretch=no
Compression=lzma2/max
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=admin
ChangesAssociations=yes
MinVersion=10.0

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Messages]
WelcomeLabel1={#AppTitle}
WelcomeLabel2=Draft preparation for League of Legends.%n%nThis will install [name/ver] on your computer.%n%nIt is recommended that you close all other applications before continuing.

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"
Name: "associate"; Description: "Open .dts and .dtt draft trees with {#AppName}"; GroupDescription: "File associations:"

[Files]
Source: "..\target\release\draftree.exe"; DestDir: "{app}"; DestName: "{#AppExe}"; Flags: ignoreversion
Source: "recvrve-installed"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\champion_icons\*"; DestDir: "{app}\champion_icons"; Flags: ignoreversion recursesubdirs
; Only the shipped config: the first start copies it to %APPDATA%\Recvrve\config.
Source: "..\config\*"; DestDir: "{app}\config"; Excludes: "settings.json,keybinds.json,*.bak,*.tmp,\imported_configs\*,\temp\*"; Flags: ignoreversion recursesubdirs
Source: "..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion
; The licenses of the libraries and fonts in Recvrve.exe (tools/third_party_notices.py).
Source: "..\THIRD-PARTY-NOTICES.txt"; DestDir: "{app}"; Flags: ignoreversion
; Used by the wizard only.
Source: "assets\setup-music.mp3"; Flags: dontcopy
Source: "assets\spoon-110.bmp"; Flags: dontcopy
Source: "assets\spoon-220.bmp"; Flags: dontcopy

[Icons]
Name: "{group}\{#AppName}"; Filename: "{app}\{#AppExe}"
Name: "{group}\{cm:UninstallProgram,{#AppName}}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Registry]
; .dts (compact) and .dtt (text) draft trees open in Recvrve.
Root: HKA; Subkey: "Software\Classes\.dts"; ValueType: string; ValueName: ""; ValueData: "Recvrve.DraftTree"; Flags: uninsdeletevalue; Tasks: associate
Root: HKA; Subkey: "Software\Classes\.dtt"; ValueType: string; ValueName: ""; ValueData: "Recvrve.DraftTree"; Flags: uninsdeletevalue; Tasks: associate
Root: HKA; Subkey: "Software\Classes\.dts\OpenWithProgids"; ValueType: string; ValueName: "Recvrve.DraftTree"; ValueData: ""; Flags: uninsdeletevalue; Tasks: associate
Root: HKA; Subkey: "Software\Classes\.dtt\OpenWithProgids"; ValueType: string; ValueName: "Recvrve.DraftTree"; ValueData: ""; Flags: uninsdeletevalue; Tasks: associate
Root: HKA; Subkey: "Software\Classes\Recvrve.DraftTree"; ValueType: string; ValueName: ""; ValueData: "Recvrve draft tree"; Flags: uninsdeletekey; Tasks: associate
Root: HKA; Subkey: "Software\Classes\Recvrve.DraftTree\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\{#AppExe},0"; Tasks: associate
Root: HKA; Subkey: "Software\Classes\Recvrve.DraftTree\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#AppExe}"" ""%1"""; Tasks: associate
Root: HKA; Subkey: "Software\Classes\Applications\{#AppExe}\SupportedTypes"; ValueType: string; ValueName: ".dts"; ValueData: ""; Flags: uninsdeletekey; Tasks: associate
Root: HKA; Subkey: "Software\Classes\Applications\{#AppExe}\SupportedTypes"; ValueType: string; ValueName: ".dtt"; ValueData: ""; Tasks: associate

[Run]
Filename: "{app}\{#AppExe}"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: nowait postinstall skipifsilent

[Code]
const
  // WebView2 Runtime: its id in EdgeUpdate, and Microsoft's Evergreen bootstrapper.
  WebView2Key = 'SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}';
  WebView2Url = 'https://go.microsoft.com/fwlink/p/?LinkId=2124703';
  WebView2File = 'MicrosoftEdgeWebview2Setup.exe';
  // Music volume, 0 to 1000.
  MusicVolume = 250;

var
  MusicOpen: Boolean;
  MusicMuted: Boolean;
  MuteButton: TNewButton;

// Windows' media control interface plays the mp3; no extra files needed.
function mciSendString(Command: String; ReturnString: String; ReturnLength: Cardinal;
  Callback: HWND): Cardinal;
  external 'mciSendStringW@winmm.dll stdcall';

function Mci(const Command: String): Boolean;
begin
  Result := mciSendString(Command, '', 0, 0) = 0;
end;

procedure StartMusic;
var
  Path: String;
begin
  ExtractTemporaryFile('setup-music.mp3');
  Path := ExpandConstant('{tmp}\setup-music.mp3');
  MusicOpen := Mci('open "' + Path + '" type mpegvideo alias recvrvemusic');
  if MusicOpen then
  begin
    Mci('setaudio recvrvemusic volume to ' + IntToStr(MusicVolume));
    MusicOpen := Mci('play recvrvemusic repeat');
  end;
end;

procedure MuteButtonClick(Sender: TObject);
begin
  MusicMuted := not MusicMuted;
  if MusicMuted then
  begin
    Mci('setaudio recvrvemusic off');
    MuteButton.Caption := 'Unmute music';
  end
  else
  begin
    Mci('setaudio recvrvemusic on');
    Mci('setaudio recvrvemusic volume to ' + IntToStr(MusicVolume));
    MuteButton.Caption := 'Mute music';
  end;
end;

// The spoon in the top right corner of the welcome page, which has no small image of its own.
// The picture sits in a panel of its own, kept in front: a picture placed on the page itself
// is drawn underneath the page's text, which covered it where the two overlapped.
procedure AddWelcomeImage;
var
  Holder: TPanel;
  Image: TBitmapImage;
  Size, Delta: Integer;
  FileName: String;
begin
  Size := ScaleX(110);
  if Size > 140 then
    FileName := 'spoon-220.bmp'
  else
    FileName := 'spoon-110.bmp';
  ExtractTemporaryFile(FileName);
  Holder := TPanel.Create(WizardForm);
  Holder.Parent := WizardForm.WelcomePage;
  Holder.Caption := '';
  Holder.BevelOuter := bvNone;
  Holder.SetBounds(WizardForm.WelcomePage.ClientWidth - Size - ScaleX(16), ScaleY(16), Size, Size);
  Image := TBitmapImage.Create(WizardForm);
  Image.Parent := Holder;
  Image.Align := alClient;
  Image.Stretch := True;
  Image.Bitmap.LoadFromFile(ExpandConstant('{tmp}\' + FileName));
  Holder.BringToFront;
  // Keep the text clear of it: the title beside it, and the text below the title wherever it
  // would still reach under the picture.
  WizardForm.WelcomeLabel1.Width := Holder.Left - WizardForm.WelcomeLabel1.Left - ScaleX(8);
  Delta := WizardForm.AdjustLabelHeight(WizardForm.WelcomeLabel1);
  WizardForm.WelcomeLabel2.Top := WizardForm.WelcomeLabel2.Top + Delta;
  if WizardForm.WelcomeLabel2.Top < Holder.Top + Holder.Height + ScaleY(8) then
  begin
    WizardForm.WelcomeLabel2.Width := WizardForm.WelcomeLabel1.Width;
    WizardForm.AdjustLabelHeight(WizardForm.WelcomeLabel2);
  end;
end;

procedure InitializeWizard;
begin
  AddWelcomeImage;
  if WizardSilent then
    Exit;
  StartMusic;
  if MusicOpen then
  begin
    MuteButton := TNewButton.Create(WizardForm);
    MuteButton.Parent := WizardForm;
    MuteButton.Caption := 'Mute music';
    MuteButton.Width := ScaleX(110);
    MuteButton.Height := WizardForm.CancelButton.Height;
    MuteButton.Left := ScaleX(12);
    MuteButton.Top := WizardForm.CancelButton.Top;
    MuteButton.OnClick := @MuteButtonClick;
  end;
end;

procedure DeinitializeSetup;
begin
  if MusicOpen then
    Mci('close recvrvemusic');
end;

function WebView2Installed: Boolean;
var
  Version: String;
begin
  Result :=
    (RegQueryStringValue(HKLM32, WebView2Key, 'pv', Version) and (Version <> '') and (Version <> '0.0.0.0'))
    or (RegQueryStringValue(HKCU, WebView2Key, 'pv', Version) and (Version <> '') and (Version <> '0.0.0.0'));
end;

function OnDownloadProgress(const Url, FileName: String; const Progress, ProgressMax: Int64): Boolean;
begin
  if ProgressMax > 0 then
    WizardForm.StatusLabel.Caption := 'Downloading the Microsoft Edge WebView2 Runtime ('
      + IntToStr(Progress * 100 div ProgressMax) + '%)...';
  Result := True;
end;

// Recvrve's window runs on WebView2. Windows 11 and up-to-date Windows 10 have it; otherwise
// Microsoft's bootstrapper downloads and installs it.
procedure InstallWebView2;
var
  ResultCode: Integer;
begin
  WizardForm.StatusLabel.Caption := 'Downloading the Microsoft Edge WebView2 Runtime...';
  try
    DownloadTemporaryFile(WebView2Url, WebView2File, '', @OnDownloadProgress);
    WizardForm.StatusLabel.Caption := 'Installing the Microsoft Edge WebView2 Runtime...';
    if not Exec(ExpandConstant('{tmp}\' + WebView2File), '/silent /install', '', SW_HIDE,
      ewWaitUntilTerminated, ResultCode) or (ResultCode <> 0) then
      SuppressibleMsgBox('The Microsoft Edge WebView2 Runtime could not be installed (code '
        + IntToStr(ResultCode) + '). Recvrve needs it to open; you can get it from '
        + 'https://developer.microsoft.com/microsoft-edge/webview2/', mbError, MB_OK, IDOK);
  except
    SuppressibleMsgBox('The Microsoft Edge WebView2 Runtime could not be downloaded: '
      + GetExceptionMessage + #13#10#13#10 + 'Recvrve needs it to open; you can get it from '
      + 'https://developer.microsoft.com/microsoft-edge/webview2/', mbError, MB_OK, IDOK);
  end;
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if (CurStep = ssPostInstall) and not WebView2Installed then
    InstallWebView2;
end;
