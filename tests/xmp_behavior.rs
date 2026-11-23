use autoshade::recipe::{
    ColourField, EditRecipe, LocalAdjustment, MaskCombine, MaskComponent, MaskGeometry, MaskRole,
};

fn without_autoshade_attributes(mut xmp: String) -> String {
    while let Some(start) = xmp.find(" ash:") {
        let value = start + xmp[start..].find("=\"").unwrap() + 2;
        let end = value + xmp[value..].find('"').unwrap() + 1;
        xmp.replace_range(start..end, "");
    }
    xmp
}

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
fn composed_mask_coverage_is_unchanged_by_an_xmp_round_trip() {
    let reference = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
        127,
        73,
        image::Rgb([128, 128, 128]),
    ));

    for mode in [
        MaskCombine::Add,
        MaskCombine::Subtract,
        MaskCombine::Intersect,
    ] {
        for inverted in [false, true] {
            let mask = LocalAdjustment {
                mask: MaskGeometry::Linear {
                    zero_x: 0.25,
                    zero_y: 0.0,
                    full_x: 0.75,
                    full_y: 1.0,
                },
                components: vec![MaskComponent {
                    geometry: MaskGeometry::Linear {
                        zero_x: 0.5,
                        zero_y: 0.25,
                        full_x: 0.5,
                        full_y: 0.75,
                    },
                    mode,
                    inverted,
                }],
                ..LocalAdjustment::default()
            };
            let recipe = EditRecipe {
                masks: vec![mask.clone()],
                ..EditRecipe::default()
            };
            let restored = autoshade::xmp::xmp_to_recipe(&autoshade::xmp::recipe_to_xmp(&recipe));

            assert_eq!(restored.masks.len(), 1, "mode={mode:?} inverted={inverted}");
            assert_eq!(
                autoshade::render::mask_coverage(
                    &mask,
                    &reference,
                    autoshade::render::MaskFrame::AsRendered,
                ),
                autoshade::render::mask_coverage(
                    &restored.masks[0],
                    &reference,
                    autoshade::render::MaskFrame::AsRendered,
                ),
                "mode={mode:?} inverted={inverted}",
            );
        }
    }
}

#[test]
fn native_xmp_preserves_mixed_component_order_in_both_whole_inversions() {
    let reference = image::DynamicImage::new_rgb8(127, 73);
    let radial = |flipped| MaskGeometry::Radial {
        top: 0.2,
        left: 0.2,
        bottom: 0.8,
        right: 0.8,
        feather: 0.5,
        roundness: 0.0,
        flipped,
        angle: 0.0,
        midpoint: 50.0,
        mask_version: 2,
    };
    let linear = || MaskGeometry::Linear {
        zero_x: 0.1,
        zero_y: 0.2,
        full_x: 0.9,
        full_y: 0.8,
    };

    for inverted in [false, true] {
        let original = LocalAdjustment {
            mask: linear(),
            inverted,
            components: vec![
                MaskComponent {
                    geometry: radial(false),
                    mode: MaskCombine::Subtract,
                    inverted: false,
                },
                MaskComponent {
                    geometry: linear(),
                    mode: MaskCombine::Add,
                    inverted: true,
                },
                MaskComponent {
                    geometry: radial(true),
                    mode: MaskCombine::Intersect,
                    inverted: false,
                },
            ],
            ..LocalAdjustment::default()
        };
        let native = without_autoshade_attributes(autoshade::xmp::recipe_to_xmp(&EditRecipe {
            masks: vec![original.clone()],
            ..EditRecipe::default()
        }));
        if !inverted {
            assert_eq!(
                native.matches("crs:MaskBlendMode=\"1\"").count(),
                2,
                "Subtract and Intersect must keep Lightroom's native mode-1 spelling"
            );
        }
        let restored = autoshade::xmp::xmp_to_recipe(&native);

        assert_eq!(restored.masks[0].components, original.components);
        assert_eq!(
            autoshade::render::mask_coverage(
                &restored.masks[0],
                &reference,
                autoshade::render::MaskFrame::AsRendered,
            ),
            autoshade::render::mask_coverage(
                &original,
                &reference,
                autoshade::render::MaskFrame::AsRendered,
            )
        );
    }
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
