use apple::wire::ImageResource;

#[test]
fn image_size_uses_variant_instead_of_index_descriptor() {
    let image: ImageResource = serde_json::from_value(serde_json::json!({
        "configuration": {
            "name": "docker.io/library/postgres:17-alpine",
            "descriptor": { "size": 10301 }
        },
        "variants": [{ "size": 115020364 }]
    }))
    .unwrap();

    assert_eq!(image.into_model().size, 115020364);
}

#[test]
fn image_size_falls_back_to_descriptor_without_variant_size() {
    let image: ImageResource = serde_json::from_value(serde_json::json!({
        "configuration": { "descriptor": { "size": 10301 } }
    }))
    .unwrap();

    assert_eq!(image.into_model().size, 10301);
}
