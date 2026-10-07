use engine::machines::media::catalogue;

fn xml(arch: &str, language: &str, url: &str, hash: &str) -> String {
    format!(
        r#"<MCT><Catalogs><Catalog><PublishedMedia><Files><File>
<FileName>Windows.esd</FileName><LanguageCode>{language}</LanguageCode>
<Architecture>{arch}</Architecture><Edition>Professional</Edition>
<Size>4527171158</Size><Sha1>{hash}</Sha1><FilePath>{url}</FilePath>
</File></Files></PublishedMedia></Catalog></Catalogs></MCT>"#
    )
}

#[test]
fn accepts_microsoft_arm64_media_with_catalogue_integrity_metadata() {
    let media = catalogue(&xml(
        "ARM64",
        "en-us",
        "http://dl.delivery.mp.microsoft.com/files/windows.esd",
        &"a".repeat(40),
    ))
    .unwrap();
    assert_eq!(media.size, 4527171158);
    assert_eq!(media.architecture, "ARM64");
}

#[test]
fn rejects_other_architectures_languages_and_untrusted_downloads() {
    for (arch, language, url, hash) in [
        (
            "x64",
            "en-us",
            "http://dl.delivery.mp.microsoft.com/files/windows.esd",
            "a".repeat(40),
        ),
        (
            "ARM64",
            "en-gb",
            "http://dl.delivery.mp.microsoft.com/files/windows.esd",
            "a".repeat(40),
        ),
        (
            "ARM64",
            "en-us",
            "https://example.com/windows.esd",
            "a".repeat(40),
        ),
        (
            "ARM64",
            "en-us",
            "https://dl.delivery.mp.microsoft.com.evil.invalid/windows.esd",
            "a".repeat(40),
        ),
        (
            "ARM64",
            "en-us",
            "http://dl.delivery.mp.microsoft.com/files/windows.esd",
            "not-a-hash".into(),
        ),
    ] {
        assert!(catalogue(&xml(arch, language, url, &hash)).is_err());
    }
}
