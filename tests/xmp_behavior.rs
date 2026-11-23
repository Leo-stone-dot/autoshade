use autoshade::recipe::{
    ColourField, EditRecipe, LocalAdjustment, MaskCombine, MaskComponent, MaskGeometry,
    MaskRole,
};

fn complex_recipe() -> EditRecipe {
    let mut recipe = EditRecipe {
        exposure_ev: -0.8034,
        contrast: 12.0,
        temperature_k: Some(5600.0),
        as_shot_k: Some(5200.0),
        rationale: "measured synthetic develop".to_string(),
        confidence: 0.8,
        colour_field: Some(ColourField {
            x: 2,
            y: 2,
            b: 2,
            grid: vec![[0.1, 0.0, 0.05, -0.02, 0.0]; 8],
            amount: 0.9,
            enabled: true,
        }),
        masks: vec![LocalAdjustment {
            name: "muted sky".to_string(),
            role: MaskRole::ZoneSky,
            enabled: false,
            mask: MaskGeometry::Radial {
                top: 0.25,
                left: 0.25,
                bottom: 0.75,
                right: 0.75,
                feather: 50.0,
                roundness: 0.0,
                flipped: false,
                angle: 12.0,
                midpoint: 50.0,
                mask_version: 2,
            },
            components: vec![MaskComponent {
                geometry: MaskGeometry::Linear {
                    zero_x: 0.1,
                    zero_y: 0.25,
                    full_x: 0.9,
                    full_y: 0.75,
                },
                mode: MaskCombine::Intersect,
                inverted: true,
            }],
            exposure_ev: -0.35,
            color_gains: Some([1.18, 0.96, 0.85]),
            ..LocalAdjustment::default()
        }],
        ..EditRecipe::default()
    };
    recipe.clamp();
    recipe
}

#[test]
fn a_fresh_xmp_document_round_trips_the_complete_develop() {
    let recipe = complex_recipe();

    let xmp = autoshade::xmp::recipe_to_xmp(&recipe);
    let restored = autoshade::xmp::xmp_to_recipe(&xmp);

    assert_eq!(restored, recipe);
}

#[test]
fn a_corrupt_payload_is_disclosed_and_the_crs_projection_still_opens() {
    let recipe = EditRecipe {
        exposure_ev: 0.5,
        colour_field: Some(ColourField {
            x: 1,
            y: 1,
            b: 1,
            grid: vec![[0.1, 0.0, 0.0, 0.0, 0.0]],
            amount: 1.0,
            enabled: true,
        }),
        ..EditRecipe::default()
    };
    let xmp = autoshade::xmp::recipe_to_xmp(&recipe);
    let marker = "asr:Recipe=\"";
    let start = xmp.find(marker).expect("our XMP carries a payload") + marker.len();
    let mut tampered = xmp.into_bytes();
    tampered[start] = if tampered[start] == b'A' { b'B' } else { b'A' };
    let tampered = String::from_utf8(tampered).unwrap();
    let collector = autoshade::diag::Collector::new();
    let diag = autoshade::diag::Diag::pixels_only(&collector);

    let restored = autoshade::xmp::xmp_to_recipe_with_diag(&tampered, &diag);
    let lines = collector.take();

    assert_eq!(restored.exposure_ev, 0.5);
    assert!(
        restored.colour_field.is_none(),
        "an untrusted payload leaked its private-only field"
    );
    assert!(
        lines
            .iter()
            .any(|line| line.text.contains("payload") || line.text.contains("checksum")),
        "the corrupt payload was not disclosed: {lines:?}"
    );
}

#[test]
fn merging_around_a_foreign_prefix_preserves_both_namespaces() {
    let foreign = "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\
<rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\
<rdf:Description rdf:about=\"\" \
xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\" \
xmlns:asr=\"urn:somebody-else\" asr:Thing=\"kept\" \
crs:Exposure2012=\"+0.10\" crs:HasSettings=\"True\"/>\
</rdf:RDF></x:xmpmeta>";
    let recipe = complex_recipe();

    let merged = autoshade::xmp::merge_recipe_into_xmp(foreign, &recipe)
        .expect("the foreign prefix does not make the XMP unmergeable")
        .doc;
    let restored = autoshade::xmp::xmp_to_recipe(&merged);

    assert!(merged.contains("xmlns:asr=\"urn:somebody-else\""));
    assert!(merged.contains("asr:Thing=\"kept\""));
    assert!(merged.contains("xmlns:asr1=\"https://autoshade.dev/ns/recipe/1.0/\""));
    assert_eq!(restored, recipe);
}
