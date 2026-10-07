use engine::machines::windows::{catalog, media::cache, setup::Tools};
use std::{
  os::unix::fs::PermissionsExt,
  path::{Path, PathBuf},
};

pub struct Fixture {
  pub root: tempfile::TempDir,
  pub tools: Tools,
}

impl Fixture {
  pub fn new() -> Self {
    let root = tempfile::tempdir().unwrap();
    for name in ["archive", "wim", "image"] {
      let path = root.path().join(name);
      std::fs::write(&path, include_str!("tool.py")).unwrap();
      std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    std::fs::write(root.path().join("mode"), "normal").unwrap();
    std::fs::write(
      root.path().join("catalog.xml"),
      format!(
        "<MCT><Catalogs><Catalog><PublishedMedia><Files><File>\
       <LanguageCode>en-us</LanguageCode><Architecture>ARM64</Architecture>\
       <Edition>Professional</Edition><Size>4527171158</Size><Sha1>{}</Sha1>\
       <FilePath>https://dl.delivery.mp.microsoft.com/files/windows.esd</FilePath>\
       </File></Files></PublishedMedia></Catalog></Catalogs></MCT>",
        "a".repeat(40),
      ),
    )
    .unwrap();
    let tools = Tools {
      archive: root.path().join("archive"),
      wim: root.path().join("wim"),
      image: root.path().join("image"),
    };
    Self { root, tools }
  }

  pub async fn selection(&self) -> catalog::Selection {
    catalog::decode(b"synthetic catalogue CAB", &self.tools.archive)
      .await
      .unwrap()
  }

  pub fn stage(&self, name: &str) -> PathBuf {
    let folder = self.root.path().join(name);
    cache::directory(&folder).unwrap();
    let iso = folder.join("installer.iso");
    std::fs::write(&iso, [1; 32]).unwrap();
    std::fs::set_permissions(&iso, std::fs::Permissions::from_mode(0o600)).unwrap();
    folder
  }

  pub async fn seed(&self, root: &Path) -> PathBuf {
    let mut folder = root.to_owned();
    for part in ["native", "images", "windows"] {
      folder.push(part);
      cache::directory(&folder).unwrap();
    }
    let stage = self.stage("stage");
    cache::publish(
      &stage,
      &folder.join("installer"),
      &self.selection().await,
      &self.tools.archive,
    )
    .await
    .unwrap()
  }
}
