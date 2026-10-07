use anyhow::ensure;
use uuid::Uuid;

#[derive(Clone, Copy)]
pub enum Role {
  User,
  Administrator,
}

pub struct Accounts {
  user: String,
  administrator: String,
}

pub struct Provision {
  pub answer: String,
  pub specialize: String,
  pub firstlogon: String,
}

/// new guest credentials only; never reuse host or registry credentials for a VM.
pub fn accounts() -> Accounts {
  Accounts {
    user: format!("Hpr7!{}", Uuid::new_v4().simple()),
    administrator: format!("Hpr7!{}", Uuid::new_v4().simple()),
  }
}

pub fn password(accounts: &Accounts, role: Role) -> &str {
  match role {
    Role::User => &accounts.user,
    Role::Administrator => &accounts.administrator,
  }
}

/// the returned answer file contains secrets and must remain private to this VM.
pub fn prepare(id: &str, accounts: &Accounts) -> anyhow::Result<Provision> {
  super::super::validate_id(id)?;
  ensure!(
    accounts.user != accounts.administrator,
    "Guest accounts need distinct passwords"
  );
  let identity = Uuid::parse_str(id)?;
  let mut digest = sha1_smol::Sha1::new();
  digest.update(identity.as_bytes());
  let computer = format!("H{}", &digest.digest().to_string()[..14]);
  let user = quick_xml::escape::escape(&accounts.user);
  let administrator = quick_xml::escape::escape(&accounts.administrator);
  let answer = format!(
    r#"<?xml version="1.0" encoding="utf-8"?>
<unattend xmlns="urn:schemas-microsoft-com:unattend" xmlns:wcm="http://schemas.microsoft.com/WMIConfig/2002/State">
  <settings pass="specialize">
    <component name="Microsoft-Windows-Shell-Setup" processorArchitecture="arm64" publicKeyToken="31bf3856ad364e35" language="neutral" versionScope="nonSxS">
      <ComputerName>{computer}</ComputerName>
    </component>
    <component name="Microsoft-Windows-Deployment" processorArchitecture="arm64" publicKeyToken="31bf3856ad364e35" language="neutral" versionScope="nonSxS">
      <RunSynchronous><RunSynchronousCommand wcm:action="add">
        <Order>1</Order>
        <Path>powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File %WINDIR%\Setup\Scripts\hopperspecialize.ps1</Path>
      </RunSynchronousCommand></RunSynchronous>
    </component>
  </settings>
  <settings pass="oobeSystem">
    <component name="Microsoft-Windows-International-Core" processorArchitecture="arm64" publicKeyToken="31bf3856ad364e35" language="neutral" versionScope="nonSxS">
      <InputLocale>0409:00000409</InputLocale><SystemLocale>en-US</SystemLocale>
      <UILanguage>en-US</UILanguage><UserLocale>en-US</UserLocale>
    </component>
    <component name="Microsoft-Windows-Shell-Setup" processorArchitecture="arm64" publicKeyToken="31bf3856ad364e35" language="neutral" versionScope="nonSxS">
      <OOBE><HideEULAPage>true</HideEULAPage><HideOnlineAccountScreens>true</HideOnlineAccountScreens><HideWirelessSetupInOOBE>true</HideWirelessSetupInOOBE><ProtectYourPC>1</ProtectYourPC></OOBE>
      <UserAccounts><LocalAccounts>
        <LocalAccount wcm:action="add"><Name>hopper</Name><DisplayName>Hopper</DisplayName><Group>Users</Group><Password><Value>{user}</Value><PlainText>true</PlainText></Password></LocalAccount>
        <LocalAccount wcm:action="add"><Name>hopperadmin</Name><DisplayName>Hopper Administrator</DisplayName><Group>Administrators</Group><Password><Value>{administrator}</Value><PlainText>true</PlainText></Password></LocalAccount>
      </LocalAccounts></UserAccounts>
      <AutoLogon><Domain>{computer}</Domain><Username>hopper</Username><Enabled>true</Enabled><LogonCount>1</LogonCount><Password><Value>{user}</Value><PlainText>true</PlainText></Password></AutoLogon>
    </component>
  </settings>
</unattend>
"#
  );
  let specialize = r#"$ErrorActionPreference = 'Stop'
$folder = Join-Path $env:ProgramData 'Hopper'
New-Item -ItemType Directory -Path $folder -Force | Out-Null
& icacls.exe $folder /inheritance:r /grant:r '*S-1-5-18:(OI)(CI)F' '*S-1-5-32-544:(OI)(CI)F' '*S-1-5-32-545:(OI)(CI)RX' | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'Cannot secure guest provisioning state' }
$script = Join-Path $env:WINDIR 'Setup\Scripts\hopperfirstlogon.ps1'
$executable = Join-Path $env:WINDIR 'System32\WindowsPowerShell\v1.0\powershell.exe'
$action = New-ScheduledTaskAction -Execute $executable -Argument ('-NoProfile -NonInteractive -ExecutionPolicy Bypass -File "' + $script + '"')
$trigger = New-ScheduledTaskTrigger -AtLogOn
$trigger.Delay = 'PT10S'
$principal = New-ScheduledTaskPrincipal -UserId 'S-1-5-18' -LogonType ServiceAccount -RunLevel Highest
$settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -StartWhenAvailable -ExecutionTimeLimit (New-TimeSpan -Minutes 2)
Register-ScheduledTask -TaskName 'HopperFirstLogon' -Action $action -Trigger $trigger -Principal $principal -Settings $settings -Force | Out-Null
"#.replace('\n', "\r\n");
  let firstlogon = r#"$ErrorActionPreference = 'Stop'
if ([Security.Principal.WindowsIdentity]::GetCurrent().User.Value -ne 'S-1-5-18') { throw 'Credential cleanup requires SYSTEM' }
$desktop = (Get-CimInstance -ClassName Win32_ComputerSystem -OperationTimeoutSec 5).UserName
if ($desktop -ne ($env:COMPUTERNAME + '\hopper')) { exit 0 }
$winlogon = 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon'
Set-ItemProperty -LiteralPath $winlogon -Name AutoLogonCount -Value 0
Set-ItemProperty -LiteralPath $winlogon -Name AutoAdminLogon -Value '0'
$properties = (Get-ItemProperty -LiteralPath $winlogon).PSObject.Properties.Name
if ($properties -contains 'DefaultPassword') { Remove-ItemProperty -LiteralPath $winlogon -Name DefaultPassword }
foreach ($path in @('Panther\unattend.xml', 'Panther\Unattend\unattend.xml')) {
  $answer = Join-Path $env:WINDIR $path
  if (Test-Path -LiteralPath $answer) { Remove-Item -LiteralPath $answer -Force }
}
$marker = Join-Path $env:ProgramData 'Hopper\firstlogon.json'
$status = @{ phase = 'firstLogon'; credentialCleanup = 'complete' } | ConvertTo-Json -Compress
[IO.File]::WriteAllText($marker, $status)
Unregister-ScheduledTask -TaskName 'HopperFirstLogon' -Confirm:$false
"#.replace('\n', "\r\n");
  Ok(Provision {
    answer,
    specialize,
    firstlogon,
  })
}
