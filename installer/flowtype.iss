#define AppName "FlowType"
#define AppVersion "0.2.7"
#define AppPublisher "FlowType"
#ifndef BuildDir
#define BuildDir "..\windows\target\release"
#endif
#define AppExeName "flowtype.exe"
#define InjectorExeName "flowtype-injector.exe"
#ifndef TipDllHash
#define TipDllHash "dev"
#endif
#define TipDllSourceName "flowtype_tip.dll"
#define TipDllName "flowtype_tip-" + TipDllHash + ".dll"
#ifndef TipDllX86Hash
#define TipDllX86Hash "dev"
#endif
#define TipDllX86SourceName "flowtype_tip_x86.dll"
#define TipDllX86Name "flowtype_tip_x86-" + TipDllX86Hash + ".dll"
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
DisableDirPage=yes
DisableProgramGroupPage=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "chinesesimplified"; MessagesFile: "languages\ChineseSimplified.isl"

[Files]
Source: "{#BuildDir}\{#AppExeName}"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#BuildDir}\{#InjectorExeName}"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#BuildDir}\{#TipDllSourceName}"; DestDir: "{app}"; DestName: "{#TipDllName}"; Flags: ignoreversion onlyifdoesntexist
Source: "{#BuildDir}\{#TipDllX86SourceName}"; DestDir: "{app}"; DestName: "{#TipDllX86Name}"; Flags: ignoreversion onlyifdoesntexist

[InstallDelete]
Type: files; Name: "{app}\flowtype-app.exe"
Type: files; Name: "{app}\flowtype-injector.exe"

[Icons]
Name: "{group}\{#AppName}"; Filename: "{app}\{#AppExeName}"; Parameters: "--show"

[Run]
Filename: "{sys}\netsh.exe"; Parameters: "advfirewall firewall delete rule name=""{#FirewallRule}"""; Flags: runhidden waituntilterminated
Filename: "{sys}\netsh.exe"; Parameters: "advfirewall firewall add rule name=""{#FirewallRule}"" dir=in action=allow protocol=TCP localport=32187 program=""{app}\{#AppExeName}"" profile=any remoteip=LocalSubnet,100.64.0.0/10 edge=no"; Flags: runhidden waituntilterminated
Filename: "{app}\{#AppExeName}"; Parameters: "--enable-auto-start"; Flags: runhidden waituntilterminated runasoriginaluser
Filename: "{app}\{#AppExeName}"; Parameters: "--show"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: nowait postinstall skipifsilent runasoriginaluser

[UninstallRun]
Filename: "{sys}\schtasks.exe"; Parameters: "/End /TN ""{#TaskName}"""; Flags: runhidden waituntilterminated; RunOnceId: "StopInjector"
Filename: "{sys}\schtasks.exe"; Parameters: "/Delete /F /TN ""{#TaskName}"""; Flags: runhidden waituntilterminated; RunOnceId: "DeleteInjectorTask"
Filename: "{sys}\netsh.exe"; Parameters: "advfirewall firewall delete rule name=""{#FirewallRule}"""; Flags: runhidden waituntilterminated; RunOnceId: "DeleteFirewallRule"

[Code]
procedure DeleteKeyboardCategory(RootKey: Integer);
var
  TipRoot: String;
  KeyboardCategory: String;
  TipClsid: String;
begin
  TipClsid := '{9A50B266-9E86-4FF4-871B-8D47AD8C658B}';
  KeyboardCategory := '{34745C63-B2F0-4784-8B67-5E12C8701A31}';
  TipRoot := 'Software\Microsoft\CTF\TIP\' + TipClsid;
  RegDeleteKeyIncludingSubkeys(RootKey,
    TipRoot + '\Category\Category\' + KeyboardCategory);
  RegDeleteKeyIncludingSubkeys(RootKey,
    TipRoot + '\Category\Item\' + TipClsid + '\' + KeyboardCategory);
end;

procedure CleanupKeyboardCategoryRegistrations();
begin
  DeleteKeyboardCategory(HKEY_LOCAL_MACHINE_64);
  DeleteKeyboardCategory(HKEY_LOCAL_MACHINE_32);
end;

procedure CleanupUserTipOverrides();
var
  TipClsid: String;
  TipRoot: String;
  ClassRoot: String;
begin
  TipClsid := '{9A50B266-9E86-4FF4-871B-8D47AD8C658B}';
  TipRoot := 'Software\Microsoft\CTF\TIP\' + TipClsid;
  ClassRoot := 'Software\Classes\CLSID\' + TipClsid;
  RegDeleteKeyIncludingSubkeys(HKEY_CURRENT_USER_64, TipRoot);
  RegDeleteKeyIncludingSubkeys(HKEY_CURRENT_USER_32, TipRoot);
  RegDeleteKeyIncludingSubkeys(HKEY_CURRENT_USER_64, ClassRoot);
  RegDeleteKeyIncludingSubkeys(HKEY_CURRENT_USER_32, ClassRoot);
end;

procedure CleanupAllTipRegistrations();
var
  TipClsid: String;
  TipRoot: String;
  ClassRoot: String;
begin
  TipClsid := '{9A50B266-9E86-4FF4-871B-8D47AD8C658B}';
  TipRoot := 'Software\Microsoft\CTF\TIP\' + TipClsid;
  ClassRoot := 'Software\Classes\CLSID\' + TipClsid;
  RegDeleteKeyIncludingSubkeys(HKEY_LOCAL_MACHINE_64, TipRoot);
  RegDeleteKeyIncludingSubkeys(HKEY_LOCAL_MACHINE_32, TipRoot);
  RegDeleteKeyIncludingSubkeys(HKEY_LOCAL_MACHINE_64, ClassRoot);
  RegDeleteKeyIncludingSubkeys(HKEY_LOCAL_MACHINE_32, ClassRoot);
  CleanupUserTipOverrides();
end;

procedure UnregisterInstalledTipDlls();
var
  FindRec: TFindRec;
  Candidate: String;
  Regsvr32: String;
  ResultCode: Integer;
begin
  if FindFirst(ExpandConstant('{app}\flowtype_tip*.dll'), FindRec) then
  begin
    try
      repeat
        Candidate := AddBackslash(ExpandConstant('{app}')) + FindRec.Name;
        if Pos('flowtype_tip_x86', Lowercase(FindRec.Name)) = 1 then
          Regsvr32 := ExpandConstant('{syswow64}\regsvr32.exe')
        else
          Regsvr32 := ExpandConstant('{sys}\regsvr32.exe');
        Exec(Regsvr32, '/u /s "' + Candidate + '"', '', SW_HIDE,
          ewWaitUntilTerminated, ResultCode);
      until not FindNext(FindRec);
    finally
      FindClose(FindRec);
    end;
  end;
end;

procedure CleanupTipDlls(KeepCurrent: Boolean);
var
  FindRec: TFindRec;
  Candidate: String;
  Keep: Boolean;
begin
  { A TSF host may still have an older TIP DLL loaded. Never schedule a reboot
    for cleanup; an occupied but unregistered file is harmless. }
  if FindFirst(ExpandConstant('{app}\flowtype_tip*.dll'), FindRec) then
  begin
    try
      repeat
        Keep := KeepCurrent and
          ((CompareText(FindRec.Name, '{#TipDllName}') = 0) or
           (CompareText(FindRec.Name, '{#TipDllX86Name}') = 0));
        if not Keep then
        begin
          Candidate := AddBackslash(ExpandConstant('{app}')) + FindRec.Name;
          DeleteFile(Candidate);
        end;
      until not FindNext(FindRec);
    finally
      FindClose(FindRec);
    end;
  end;
end;

procedure RegisterCurrentTipDlls();
var
  ResultCode: Integer;
  TipX86: String;
  TipX64: String;
begin
  TipX86 := ExpandConstant('{app}\{#TipDllX86Name}');
  TipX64 := ExpandConstant('{app}\{#TipDllName}');
  if (not Exec(ExpandConstant('{syswow64}\regsvr32.exe'),
      '/s "' + TipX86 + '"', '', SW_HIDE, ewWaitUntilTerminated, ResultCode)) or
      (ResultCode <> 0) then
    RaiseException('Failed to register the FlowType x86 text service.');
  if (not Exec(ExpandConstant('{sys}\regsvr32.exe'),
      '/s "' + TipX64 + '"', '', SW_HIDE, ewWaitUntilTerminated, ResultCode)) or
      (ResultCode <> 0) then
  begin
    CleanupAllTipRegistrations();
    RaiseException('Failed to register the FlowType x64 text service.');
  end;
end;

procedure ConfigureInjectorTask();
var
  ResultCode: Integer;
  Parameters: String;
begin
  Parameters := '/Create /F /TN "{#TaskName}" /SC ONLOGON /RL HIGHEST /IT ' +
    '/TR %ProgramFiles%\FlowType\{#InjectorExeName}';
  if (not Exec(ExpandConstant('{sys}\schtasks.exe'), Parameters, '', SW_HIDE,
      ewWaitUntilTerminated, ResultCode)) or (ResultCode <> 0) then
    RaiseException('Failed to create the FlowType Injector task.');
  Parameters := '-NoProfile -NonInteractive -ExecutionPolicy Bypass -Command "' +
    '$settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries ' +
    '-DontStopIfGoingOnBatteries -ExecutionTimeLimit ([TimeSpan]::Zero); ' +
    'Set-ScheduledTask -TaskName ' + #39 + '{#TaskName}' + #39 +
    ' -Settings $settings | Out-Null"';
  if (not Exec(ExpandConstant('{sys}\WindowsPowerShell\v1.0\powershell.exe'),
      Parameters, '', SW_HIDE, ewWaitUntilTerminated, ResultCode)) or
      (ResultCode <> 0) then
    RaiseException('Failed to configure the FlowType Injector task.');
  if (not Exec(ExpandConstant('{sys}\schtasks.exe'), '/Run /TN "{#TaskName}"',
      '', SW_HIDE, ewWaitUntilTerminated, ResultCode)) or (ResultCode <> 0) then
    RaiseException('Failed to start the FlowType Injector task.');
end;

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

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  UnregisterInstalledTipDlls();
  CleanupUserTipOverrides();
  CleanupKeyboardCategoryRegistrations();
  Result := '';
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
  begin
    RegisterCurrentTipDlls();
    CleanupKeyboardCategoryRegistrations();
    ConfigureInjectorTask();
    CleanupTipDlls(True);
  end;
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

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usUninstall then
  begin
    UnregisterInstalledTipDlls();
    CleanupAllTipRegistrations();
  end
  else if CurUninstallStep = usPostUninstall then
  begin
    CleanupAllTipRegistrations();
    CleanupTipDlls(False);
  end;
end;
