#define AppName "FlowType"
#define AppVersion "0.1.15"
#define AppPublisher "FlowType"
#ifndef BuildDir
#define BuildDir "..\windows\target\release"
#endif
#define AppExeName "flowtype.exe"
#define InjectorExeName "flowtype-injector.exe"
#define TipDllName "flowtype_tip_0_1_15.dll"
#define TipDllX86Name "flowtype_tip_x86_0_1_15.dll"
#define TaskName "FlowType Injector"
#define FirewallRule "FlowType Local Network"

[Setup]
AppId={{8B7DAED3-CB08-47D9-A582-D9630C8C4516}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher={#AppPublisher}
DefaultDirName={autopf}\FlowType
DefaultGroupName={#AppName}
OutputDir=output
OutputBaseFilename=FlowType-{#AppVersion}-x64-setup
Compression=lzma2/ultra64
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=admin
UninstallDisplayName={#AppName}
WizardStyle=modern
CloseApplications=no
RestartApplications=no
ChangesEnvironment=no
DisableProgramGroupPage=yes

[Files]
Source: "{#BuildDir}\{#AppExeName}"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#BuildDir}\{#InjectorExeName}"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#BuildDir}\{#TipDllName}"; DestDir: "{app}"; Flags: ignoreversion restartreplace uninsrestartdelete
Source: "{#BuildDir}\{#TipDllX86Name}"; DestDir: "{app}"; Flags: ignoreversion restartreplace uninsrestartdelete

[InstallDelete]
Type: files; Name: "{app}\flowtype-app.exe"
Type: files; Name: "{app}\flowtype-injector.exe"

[Icons]
Name: "{group}\{#AppName}"; Filename: "{app}\{#AppExeName}"; Parameters: "--show"

[Run]
Filename: "{sys}\regsvr32.exe"; Parameters: "/s ""{app}\{#TipDllName}"""; Flags: runhidden waituntilterminated
Filename: "{syswow64}\regsvr32.exe"; Parameters: "/s ""{app}\{#TipDllX86Name}"""; Flags: runhidden waituntilterminated
Filename: "{sys}\schtasks.exe"; Parameters: "/Create /F /TN ""{#TaskName}"" /SC ONLOGON /RL HIGHEST /IT /TR """"{app}\{#InjectorExeName}"""""; Flags: runhidden waituntilterminated
Filename: "{sys}\schtasks.exe"; Parameters: "/Run /TN ""{#TaskName}"""; Flags: runhidden waituntilterminated
Filename: "{sys}\netsh.exe"; Parameters: "advfirewall firewall delete rule name=""{#FirewallRule}"""; Flags: runhidden waituntilterminated
Filename: "{sys}\netsh.exe"; Parameters: "advfirewall firewall add rule name=""{#FirewallRule}"" dir=in action=allow protocol=TCP localport=32187 program=""{app}\{#AppExeName}"" profile=any"; Flags: runhidden waituntilterminated
Filename: "{app}\{#AppExeName}"; Parameters: "--enable-auto-start"; Flags: runhidden waituntilterminated runasoriginaluser
Filename: "{app}\{#AppExeName}"; Parameters: "--show"; Description: "运行{#AppName}"; Flags: nowait postinstall skipifsilent runasoriginaluser

[UninstallRun]
Filename: "{sys}\schtasks.exe"; Parameters: "/End /TN ""{#TaskName}"""; Flags: runhidden waituntilterminated; RunOnceId: "StopInjector"
Filename: "{sys}\regsvr32.exe"; Parameters: "/u /s ""{app}\{#TipDllName}"""; Flags: runhidden waituntilterminated; RunOnceId: "UnregisterTip"
Filename: "{syswow64}\regsvr32.exe"; Parameters: "/u /s ""{app}\{#TipDllX86Name}"""; Flags: runhidden waituntilterminated; RunOnceId: "UnregisterTipX86"
Filename: "{sys}\schtasks.exe"; Parameters: "/Delete /F /TN ""{#TaskName}"""; Flags: runhidden waituntilterminated; RunOnceId: "DeleteInjectorTask"
Filename: "{sys}\netsh.exe"; Parameters: "advfirewall firewall delete rule name=""{#FirewallRule}"""; Flags: runhidden waituntilterminated; RunOnceId: "DeleteFirewallRule"

[Code]
function InitializeSetup(): Boolean;
var
  ResultCode: Integer;
begin
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/IM {#AppExeName} /F', '', SW_HIDE,
    ewWaitUntilTerminated, ResultCode);
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/IM {#InjectorExeName} /F', '', SW_HIDE,
    ewWaitUntilTerminated, ResultCode);
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/IM flowtype-app.exe /F', '', SW_HIDE,
    ewWaitUntilTerminated, ResultCode);
  Result := True;
end;

function InitializeUninstall(): Boolean;
var
  ResultCode: Integer;
begin
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/IM {#AppExeName} /F', '', SW_HIDE,
    ewWaitUntilTerminated, ResultCode);
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/IM {#InjectorExeName} /F', '', SW_HIDE,
    ewWaitUntilTerminated, ResultCode);
  RegDeleteValue(HKEY_CURRENT_USER,
    'Software\Microsoft\Windows\CurrentVersion\Run', 'FlowType');
  Result := True;
end;
