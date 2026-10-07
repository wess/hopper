use engine::machines::windows::provision;
use serde::Deserialize;

#[derive(Deserialize)]
struct Answer {
  #[serde(rename = "settings")]
  settings: Vec<Settings>,
}
#[derive(Deserialize)]
struct Settings {
  #[serde(rename = "@pass")]
  pass: String,
  #[serde(rename = "component")]
  components: Vec<Component>,
}
#[derive(Deserialize)]
struct Component {
  #[serde(rename = "@processorArchitecture")]
  architecture: String,
  #[serde(rename = "ComputerName")]
  computer: Option<String>,
  #[serde(rename = "UserAccounts")]
  accounts: Option<Users>,
  #[serde(rename = "AutoLogon")]
  login: Option<Login>,
}
#[derive(Deserialize)]
struct Users {
  #[serde(rename = "LocalAccounts")]
  accounts: Locals,
}
#[derive(Deserialize)]
struct Locals {
  #[serde(rename = "LocalAccount")]
  accounts: Vec<Account>,
}
#[derive(Deserialize)]
struct Account {
  #[serde(rename = "Name")]
  name: String,
  #[serde(rename = "Group")]
  group: String,
}
#[derive(Deserialize)]
struct Login {
  #[serde(rename = "Domain")]
  domain: String,
  #[serde(rename = "Username")]
  name: String,
  #[serde(rename = "LogonCount")]
  count: u32,
}

const ID: &str = "8197e0f0-0603-43e9-a817-eaf7ab0327af";

fn computer(answer: &str) -> String {
  let answer: Answer = quick_xml::de::from_str(answer).unwrap();
  answer
    .settings
    .into_iter()
    .flat_map(|pass| pass.components)
    .find_map(|component| component.computer)
    .unwrap()
}

#[test]
fn first_boot_uses_a_normal_desktop_account_and_distinct_administrator() {
  let accounts = provision::accounts();
  let provision = provision::prepare(ID, &accounts).unwrap();
  let answer: Answer = quick_xml::de::from_str(&provision.answer).unwrap();
  assert!(answer.settings.iter().any(|pass| pass.pass == "specialize"));
  let oobe = answer
    .settings
    .iter()
    .find(|pass| pass.pass == "oobeSystem")
    .unwrap();
  for component in answer.settings.iter().flat_map(|pass| &pass.components) {
    assert_eq!(component.architecture, "arm64");
  }
  let users = &oobe
    .components
    .iter()
    .find_map(|c| c.accounts.as_ref())
    .unwrap()
    .accounts
    .accounts;
  assert_eq!(users.len(), 2);
  assert_eq!(users[0].name, "hopper");
  assert_eq!(users[0].group, "Users");
  assert_eq!(users[1].name, "hopperadmin");
  assert_eq!(users[1].group, "Administrators");
  let login = oobe
    .components
    .iter()
    .find_map(|c| c.login.as_ref())
    .unwrap();
  assert_eq!(login.name, "hopper");
  assert_eq!(login.domain, computer(&provision.answer));
  assert_eq!(login.count, 1);
  assert!(!provision.answer.contains("ProductKey"));
  assert!(!provision.answer.contains("SkipMachineOOBE"));
}

#[test]
fn generated_passwords_are_distinct_and_never_derived_from_vm_identity() {
  let first = provision::accounts();
  let second = provision::accounts();
  for accounts in [&first, &second] {
    let user = provision::password(accounts, provision::Role::User);
    let administrator = provision::password(accounts, provision::Role::Administrator);
    assert!(user != administrator);
    for password in [user, administrator] {
      assert!(password.len() >= 32);
      assert!(password.bytes().any(|b| b.is_ascii_uppercase()));
      assert!(password.bytes().any(|b| b.is_ascii_lowercase()));
      assert!(password.bytes().any(|b| b.is_ascii_digit()));
      assert!(password.contains('!'));
    }
  }
  assert!(
    provision::password(&first, provision::Role::User)
      != provision::password(&second, provision::Role::User)
  );
}

#[test]
fn clone_identity_affects_the_hostname_even_when_uuid_prefixes_match() {
  let accounts = provision::accounts();
  let first = provision::prepare(ID, &accounts).unwrap();
  let second = provision::prepare("8197e0f0-0603-43e9-a817-eaf7ab0327b0", &accounts).unwrap();
  let name = computer(&first.answer);
  assert!(name.len() <= 15 && name.bytes().all(|b| b.is_ascii_alphanumeric()));
  assert_ne!(name, computer(&second.answer));
  let canonical = provision::prepare(&ID.to_uppercase(), &accounts).unwrap();
  assert_eq!(name, computer(&canonical.answer));
  assert!(provision::prepare("bad<&vm", &accounts).is_err());
}

#[test]
fn cleanup_runs_as_system_and_does_not_claim_desktop_or_tools_readiness() {
  let provision = provision::prepare(ID, &provision::accounts()).unwrap();
  assert!(provision
    .specialize
    .contains("-UserId 'S-1-5-18' -LogonType ServiceAccount"));
  assert!(provision
    .firstlogon
    .contains("-Name AutoLogonCount -Value 0"));
  assert!(provision
    .firstlogon
    .contains("-Name AutoAdminLogon -Value '0'"));
  assert!(provision
    .firstlogon
    .contains("Remove-ItemProperty -LiteralPath $winlogon -Name DefaultPassword"));
  assert!(provision.firstlogon.contains("Panther\\unattend.xml"));
  assert!(provision.firstlogon.contains("Unregister-ScheduledTask"));
  let guard = provision.firstlogon.find("if ($desktop -ne").unwrap();
  let cleanup = provision.firstlogon.find("Set-ItemProperty").unwrap();
  assert!(guard < cleanup);
  assert!(provision
    .firstlogon
    .contains("Credential cleanup requires SYSTEM"));
  assert!(!provision.firstlogon.contains("ready"));
  assert!(!provision.firstlogon.contains("SilentlyContinue"));
}
