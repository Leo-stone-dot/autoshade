use autoshade::recipe::{
    BrushStroke, ColourField, EditRecipe, LocalAdjustment, MaskCombine, MaskComponent,
    MaskGeometry, MaskRole,
};

fn without_autoshade_attributes(mut xmp: String) -> String {
    while let Some(start) = xmp.find(" ash:") {
        let value = start + xmp[start..].find("=\"").unwrap() + 2;
        let end = value + xmp[value..].find('"').unwrap() + 1;
        xmp.replace_range(start..end, "");
    }
    xmp
}

fn without_attribute(mut xmp: String, name: &str) -> String {
    let marker = format!(" {name}=\"");
    while let Some(start) = xmp.find(&marker) {
        let value = start + marker.len();
        let end = value + xmp[value..].find('"').unwrap() + 1;
        xmp.replace_range(start..end, "");
    }
    xmp
}

fn without_payload(xmp: String) -> String {
    without_attribute(
        without_attribute(xmp, "asr:Recipe"),
        "asr:RecipeCrc32",
    )
}

fn xmp_attribute_values<'a>(xmp: &'a str, name: &str) -> Vec<&'a str> {
    let marker = format!("{name}=\"");
    let mut values = Vec::new();
    let mut rest = xmp;
    while let Some(start) = rest.find(&marker) {
        let value = &rest[start + marker.len()..];
        let end = value.find('"').unwrap();
        values.push(&value[..end]);
        rest = &value[end + 1..];
    }
    values
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
fn native_xmp_spells_every_combine_mode_and_component_inversion() {
    let linear = || MaskGeometry::Linear {
        zero_x: 0.1,
        zero_y: 0.2,
        full_x: 0.9,
        full_y: 0.8,
    };
    for (mode, blend, value) in [
        (MaskCombine::Add, "0", "1"),
        (MaskCombine::Subtract, "1", "0"),
        (MaskCombine::Intersect, "1", "0"),
    ] {
        for inverted in [false, true] {
            let recipe = EditRecipe {
                masks: vec![LocalAdjustment {
                    mask: linear(),
                    components: vec![MaskComponent {
                        geometry: linear(),
                        mode,
                        inverted,
                    }],
                    ..LocalAdjustment::default()
                }],
                ..EditRecipe::default()
            };
            let native =
                without_autoshade_attributes(autoshade::xmp::recipe_to_xmp(&recipe));
            let blends = xmp_attribute_values(&native, "crs:MaskBlendMode");
            let inversions = xmp_attribute_values(&native, "crs:MaskInverted");
            let values = xmp_attribute_values(&native, "crs:MaskValue");

            assert_eq!(blends.last().copied(), Some(blend));
            assert_eq!(
                inversions.last().copied(),
                Some(if mode == MaskCombine::Intersect {
                    if inverted { "false" } else { "true" }
                } else if inverted {
                    "true"
                } else {
                    "false"
                })
            );
            assert_eq!(values.last().copied(), Some(value));
            assert_eq!(
                autoshade::xmp::xmp_to_recipe(&native).masks[0].components[0],
                recipe.masks[0].components[0]
            );
        }
    }
}

#[test]
fn component_inversion_is_sparse_and_legacy_json_stays_compatible() {
    let plain = MaskComponent {
        geometry: MaskGeometry::Linear {
            zero_x: 0.1,
            zero_y: 0.2,
            full_x: 0.9,
            full_y: 0.8,
        },
        mode: MaskCombine::Intersect,
        inverted: false,
    };
    let plain_json = serde_json::to_string(&plain).unwrap();
    assert!(!plain_json.contains("inverted"));
    assert_eq!(
        serde_json::from_str::<MaskComponent>(&plain_json).unwrap(),
        plain
    );

    let inverted = MaskComponent {
        inverted: true,
        ..plain
    };
    let inverted_json = serde_json::to_string(&inverted).unwrap();
    assert!(inverted_json.contains("\"inverted\":true"));
    assert_eq!(
        serde_json::from_str::<MaskComponent>(&inverted_json).unwrap(),
        inverted
    );
    assert_eq!(
        EditRecipe::default().schema_era,
        autoshade::recipe::SCHEMA_ERA
    );
}

#[test]
fn stale_radial_inversion_metadata_cannot_reverse_a_linear_mask() {
    let original = LocalAdjustment {
        mask: MaskGeometry::Linear {
            zero_x: 0.25,
            zero_y: 0.0,
            full_x: 0.75,
            full_y: 1.0,
        },
        inverted: true,
        ..LocalAdjustment::default()
    };
    let xmp = autoshade::xmp::recipe_to_xmp(&EditRecipe {
        masks: vec![original.clone()],
        ..EditRecipe::default()
    })
    .replace("ash:Inverted=\"true\"", "ash:Inverted=\"false\"")
    .replace("ash:BaseInverted=\"false\"", "ash:BaseInverted=\"true\"");

    let restored = autoshade::xmp::xmp_to_recipe(&xmp);
    let reference = image::DynamicImage::new_rgb8(127, 73);

    assert_eq!(restored.masks[0].mask, original.mask);
    assert_eq!(restored.masks[0].inverted, original.inverted);
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

#[test]
fn rotated_radial_components_use_the_frame_or_disclose_rotation_loss() {
    let radial = MaskGeometry::Radial {
        top: 0.25,
        left: 0.15,
        bottom: 0.75,
        right: 0.85,
        feather: 0.5,
        roundness: 0.0,
        flipped: false,
        angle: 24.0,
        midpoint: 50.0,
        mask_version: 2,
    };
    let recipe = EditRecipe {
        masks: vec![LocalAdjustment {
            mask: MaskGeometry::Linear {
                zero_x: 0.25,
                zero_y: 0.25,
                full_x: 0.75,
                full_y: 0.75,
            },
            components: vec![MaskComponent {
                geometry: radial,
                mode: MaskCombine::Intersect,
                inverted: false,
            }],
            ..LocalAdjustment::default()
        }],
        ..EditRecipe::default()
    };

    let (_, losses) = autoshade::xmp::recipe_to_xmp_with_losses(&recipe);
    assert!(matches!(
        losses.as_slice(),
        [autoshade::xmp::MaskLoss {
            reason: autoshade::xmp::MaskLossReason::Rotation(24),
            ..
        }]
    ));

    let (xmp, framed_losses) = autoshade::xmp::recipe_to_xmp_in_frame(
        &recipe,
        autoshade::xmp::FrameAspect::from_size(600.0, 400.0),
    );
    assert!(framed_losses.is_empty());
    let restored = autoshade::xmp::xmp_to_recipe(&xmp);
    let MaskGeometry::Radial { angle, .. } = restored.masks[0].components[0].geometry else {
        panic!("the component remains radial")
    };
    assert!((angle - 24.0).abs() < 1e-4);
}

#[test]
fn editor_intent_survives_when_the_namespace_binding_moves_to_the_root() {
    let original = LocalAdjustment {
        mask: MaskGeometry::Linear {
            zero_x: 0.25,
            zero_y: 0.25,
            full_x: 0.75,
            full_y: 0.75,
        },
        inverted: true,
        role: MaskRole::ZoneSky,
        components: vec![MaskComponent {
            geometry: MaskGeometry::Radial {
                top: 0.25,
                left: 0.25,
                bottom: 0.75,
                right: 0.75,
                feather: 0.5,
                roundness: 0.0,
                flipped: false,
                angle: 0.0,
                midpoint: 50.0,
                mask_version: 2,
            },
            mode: MaskCombine::Subtract,
            inverted: false,
        }],
        ..LocalAdjustment::default()
    };
    let document = without_payload(autoshade::xmp::recipe_to_xmp(&EditRecipe {
            masks: vec![original.clone()],
            ..EditRecipe::default()
        }));
    let declaration = " xmlns:ash=\"https://autoshade.dev/ns/mask/1.0/\"";
    let stripped = document.replace(declaration, "");
    let root = stripped.find("<rdf:Description").unwrap();
    let hoisted = format!(
        "{}<rdf:Description{}{}",
        &stripped[..root],
        declaration,
        &stripped[root + "<rdf:Description".len()..]
    );
    let restored = autoshade::xmp::xmp_to_recipe(&hoisted);
    let reference = image::DynamicImage::new_rgb8(96, 64);

    assert_eq!(restored.masks[0].role, MaskRole::ZoneSky);
    assert!(restored.masks[0].inverted);
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

    let unbound = autoshade::xmp::xmp_to_recipe(&stripped);
    assert_eq!(unbound.masks[0].role, MaskRole::Custom);
    let shadowed = hoisted.replacen(
        " ash:Role=",
        " xmlns:ash=\"https://example.invalid/other/\" ash:Role=",
        1,
    );
    assert_eq!(
        autoshade::xmp::xmp_to_recipe(&shadowed).masks[0].role,
        MaskRole::Custom
    );
}

#[test]
fn portrait_roles_round_trip_and_unknown_roles_degrade_to_custom() {
    for role in [
        MaskRole::Face,
        MaskRole::Eyes,
        MaskRole::Skin,
        MaskRole::Background,
    ] {
        let document = without_payload(autoshade::xmp::recipe_to_xmp(&EditRecipe {
                masks: vec![LocalAdjustment {
                    mask: MaskGeometry::Linear {
                        zero_x: 0.25,
                        zero_y: 0.25,
                        full_x: 0.75,
                        full_y: 0.75,
                    },
                    role,
                    exposure_ev: 0.25,
                    ..LocalAdjustment::default()
                }],
                ..EditRecipe::default()
            }));

        assert!(document.contains(&format!("ash:Role=\"{}\"", role.tag())));
        assert_eq!(autoshade::xmp::xmp_to_recipe(&document).masks[0].role, role);
        let newer = document.replace(
            &format!("ash:Role=\"{}\"", role.tag()),
            "ash:Role=\"hair\"",
        );
        assert_eq!(
            autoshade::xmp::xmp_to_recipe(&newer).masks[0].role,
            MaskRole::Custom
        );
    }
}

#[test]
fn edited_ai_component_modes_override_carried_blend_and_keep_plain_add_zero() {
    let mut ai = MaskGeometry::select_sky(0.5, 0.25, false, String::new());
    if let MaskGeometry::AiMask {
        blend_mode, value, ..
    } = &mut ai
    {
        *blend_mode = 1;
        *value = 0.0;
    }
    for mode in [
        MaskCombine::Add,
        MaskCombine::Subtract,
        MaskCombine::Intersect,
    ] {
        let recipe = EditRecipe {
            masks: vec![LocalAdjustment {
                mask: MaskGeometry::Linear {
                    zero_x: 0.25,
                    zero_y: 0.25,
                    full_x: 0.75,
                    full_y: 0.75,
                },
                components: vec![MaskComponent {
                    geometry: ai.clone(),
                    mode,
                    inverted: false,
                }],
                ..LocalAdjustment::default()
            }],
            ..EditRecipe::default()
        };
        let native = without_payload(autoshade::xmp::recipe_to_xmp(&recipe));
        let restored = autoshade::xmp::xmp_to_recipe(&native);

        assert_eq!(restored.masks[0].components[0].mode, mode);
        let MaskGeometry::AiMask {
            blend_mode, value, ..
        } = restored.masks[0].components[0].geometry
        else {
            panic!("the component remains an AI mask")
        };
        assert_eq!(blend_mode, if mode == MaskCombine::Add { 0 } else { 1 });
        assert_eq!(value, 0.0);
        assert_eq!(
            native,
            without_payload(autoshade::xmp::recipe_to_xmp(&restored))
        );
    }
}

#[test]
fn edited_brush_component_modes_keep_the_same_native_spelling() {
    let brush = MaskGeometry::Brush {
        name: "Brush 1".to_string(),
        blend_mode: 1,
        value: 0.0,
        inverted: false,
        strokes: vec![BrushStroke {
            dabs: "d 0.5 0.5".to_string(),
            radius: 0.1,
            ..BrushStroke::default()
        }],
    };
    for mode in [
        MaskCombine::Add,
        MaskCombine::Subtract,
        MaskCombine::Intersect,
    ] {
        let recipe = EditRecipe {
            masks: vec![LocalAdjustment {
                mask: MaskGeometry::Linear {
                    zero_x: 0.25,
                    zero_y: 0.25,
                    full_x: 0.75,
                    full_y: 0.75,
                },
                components: vec![MaskComponent {
                    geometry: brush.clone(),
                    mode,
                    inverted: false,
                }],
                ..LocalAdjustment::default()
            }],
            ..EditRecipe::default()
        };
        let native = without_payload(autoshade::xmp::recipe_to_xmp(&recipe));
        let restored = autoshade::xmp::xmp_to_recipe(&native);

        assert_eq!(restored.masks[0].components[0].mode, mode);
        let MaskGeometry::Brush {
            blend_mode, value, ..
        } = restored.masks[0].components[0].geometry
        else {
            panic!("the component remains a brush")
        };
        assert_eq!(blend_mode, if mode == MaskCombine::Add { 0 } else { 1 });
        assert_eq!(value, 0.0);
    }
}

#[test]
fn complementing_the_whole_mask_projects_subtract_and_intersect_as_add_one() {
    let mut ai = MaskGeometry::select_sky(0.5, 0.25, false, String::new());
    if let MaskGeometry::AiMask {
        blend_mode, value, ..
    } = &mut ai
    {
        *blend_mode = 1;
        *value = 0.0;
    }
    for mode in [MaskCombine::Subtract, MaskCombine::Intersect] {
        let recipe = EditRecipe {
            masks: vec![LocalAdjustment {
                mask: MaskGeometry::Linear {
                    zero_x: 0.25,
                    zero_y: 0.25,
                    full_x: 0.75,
                    full_y: 0.75,
                },
                inverted: true,
                components: vec![MaskComponent {
                    geometry: ai.clone(),
                    mode,
                    inverted: false,
                }],
                ..LocalAdjustment::default()
            }],
            ..EditRecipe::default()
        };
        let native = without_payload(autoshade::xmp::recipe_to_xmp(&recipe));
        let restored = autoshade::xmp::xmp_to_recipe(&native);

        assert_eq!(restored.masks[0].components[0].mode, mode);
        let MaskGeometry::AiMask {
            blend_mode, value, ..
        } = restored.masks[0].components[0].geometry
        else {
            panic!("the component remains an AI mask")
        };
        assert_eq!((blend_mode, value), (0, 1.0));
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
