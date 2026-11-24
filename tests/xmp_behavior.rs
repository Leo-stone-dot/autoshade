use autoshade::recipe::{
    BrushStroke, ColourField, EditRecipe, LocalAdjustment, MaskCombine, MaskComponent,
    LensProfile, MaskGeometry, MaskRole,
};
use autoshade::render::{LensBlur, PixelLayer};

mod support;

const LIGHTROOM_BASE: &str = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/">
<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
<rdf:Description rdf:about=""
 xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"
 crs:ProcessVersion="15.4" crs:LensProfileEnable="0" crs:HasSettings="True"
 crs:CameraProfile="Adobe Standard">
 <crs:Look><rdf:Description crs:Name="Adobe Color" crs:Amount="1"
  crs:UUID="B952C231111CD8E0ECCF14B86BAA7077">
  <crs:Parameters><rdf:Description crs:CameraProfile="Adobe Standard"
   crs:LookTable="E1095149FDB39D7A057BAB208837E2E1">
   <crs:ToneCurvePV2012><rdf:Seq>
    <rdf:li>0, 0</rdf:li><rdf:li>40, 35</rdf:li><rdf:li>127, 127</rdf:li>
    <rdf:li>224, 230</rdf:li><rdf:li>255, 255</rdf:li>
   </rdf:Seq></crs:ToneCurvePV2012>
  </rdf:Description></crs:Parameters>
 </rdf:Description></crs:Look>
</rdf:Description></rdf:RDF></x:xmpmeta>"#;

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

fn without_mask_intent(xmp: String) -> String {
    without_autoshade_attributes(xmp)
        .replace(" xmlns:ash=\"https://autoshade.dev/ns/mask/1.0/\"", "")
}

fn replace_single_native_inversion(xmp: &str, from: bool, to: bool) -> String {
    let old = format!("crs:MaskInverted=\"{from}\"");
    assert_eq!(xmp.matches(&old).count(), 1, "fixture has one base mask");
    xmp.replacen(&old, &format!("crs:MaskInverted=\"{to}\""), 1)
}

fn external_rewrite(original: &EditRecipe, edited: &EditRecipe) -> String {
    let original_xmp = autoshade::xmp::recipe_to_xmp(original);
    let mut rewritten = without_mask_intent(autoshade::xmp::recipe_to_xmp(edited));
    for name in ["asr:RecipeCrc32", "asr:Recipe"] {
        let old = xmp_attribute_values(&rewritten, name)[0];
        let new = xmp_attribute_values(&original_xmp, name)[0];
        rewritten = rewritten.replacen(
            &format!("{name}=\"{old}\""),
            &format!("{name}=\"{new}\""),
            1,
        );
    }
    rewritten
}

fn unbacked_sky_geometry() -> MaskGeometry {
    MaskGeometry::AiMask {
        name: "Sky 1".to_string(),
        subtype: 2,
        ref_x: 0.5,
        ref_y: 0.25,
        blend_mode: 0,
        value: 1.0,
        inverted: false,
        mask_version: 1,
        provenance: Vec::new(),
        gesture: Vec::new(),
        raster: None,
    }
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
fn native_composition_matrix_preserves_editor_intent_and_crs_coverage() {
    let tree = support::TempTree::new("xmp-native-composition");
    let raster = tree.path().join("sky.png");
    image::GrayImage::from_fn(96, 64, |x, y| {
        image::Luma([((x + 2 * y) % 256) as u8])
    })
    .save(&raster)
    .unwrap();
    let linear = || MaskGeometry::Linear {
        zero_x: 0.25,
        zero_y: 0.25,
        full_x: 0.75,
        full_y: 0.75,
    };
    let radial = |flipped| MaskGeometry::Radial {
        top: 0.25,
        left: 0.25,
        bottom: 0.75,
        right: 0.75,
        feather: 0.5,
        roundness: 0.0,
        flipped,
        angle: 0.0,
        midpoint: 50.0,
        mask_version: 2,
    };
    let ai = MaskGeometry::select_sky(
        0.5,
        0.25,
        false,
        raster.to_string_lossy().into_owned(),
    );
    let reference = image::DynamicImage::new_rgb8(96, 64);

    for (base, geometry, mode, component_inverted, whole_inverted) in [
        (linear(), radial(false), MaskCombine::Intersect, false, false),
        (ai.clone(), linear(), MaskCombine::Subtract, false, false),
        (radial(false), linear(), MaskCombine::Add, false, false),
        (linear(), radial(true), MaskCombine::Intersect, false, false),
        (ai.clone(), linear(), MaskCombine::Intersect, true, false),
        (linear(), radial(false), MaskCombine::Subtract, false, true),
        (linear(), radial(true), MaskCombine::Add, false, true),
        (linear(), radial(false), MaskCombine::Intersect, false, true),
    ] {
        let original = LocalAdjustment {
            mask: base,
            role: MaskRole::ZoneSky,
            inverted: whole_inverted,
            components: vec![MaskComponent {
                geometry,
                mode,
                inverted: component_inverted,
            }],
            ..LocalAdjustment::default()
        };
        let recipe = EditRecipe {
            masks: vec![original.clone()],
            ..EditRecipe::default()
        };
        let (document, losses) = autoshade::xmp::recipe_to_xmp_in_frame(
            &recipe,
            autoshade::xmp::FrameAspect::from_size(96.0, 64.0),
        );
        assert!(!losses.iter().any(|loss| matches!(
            loss.reason,
            autoshade::xmp::MaskLossReason::ComponentsFlattened
                | autoshade::xmp::MaskLossReason::Rotation(_)
        )));
        let native = without_payload(document);
        let mut restored = autoshade::xmp::xmp_to_recipe(&native);
        if let MaskGeometry::AiMask { raster: slot, .. } = &mut restored.masks[0].mask {
            *slot = Some(raster.to_string_lossy().into_owned());
        }
        assert_eq!(restored.masks[0].role, original.role);
        assert_eq!(restored.masks[0].inverted, original.inverted);
        assert_eq!(restored.masks[0].components, original.components);
        assert_eq!(restored.masks[0].mask, original.mask);

        let crs_only = without_autoshade_attributes(native);
        let mut crs = autoshade::xmp::xmp_to_recipe(&crs_only);
        if let MaskGeometry::AiMask { raster: slot, .. } = &mut crs.masks[0].mask {
            *slot = Some(raster.to_string_lossy().into_owned());
        }
        assert_eq!(
            autoshade::render::mask_coverage(
                &crs.masks[0],
                &reference,
                autoshade::render::MaskFrame::AsRendered,
            ),
            autoshade::render::mask_coverage(
                &original,
                &reference,
                autoshade::render::MaskFrame::AsRendered,
            ),
            "mode={mode:?} component_inverted={component_inverted} whole_inverted={whole_inverted}"
        );
    }
}

#[test]
fn a_census_shaped_sky_intersect_radial_imports_as_intersect() {
    let xmp = r#"<rdf:Description xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/">
<crs:MaskGroupBasedCorrections><rdf:Seq><rdf:li><rdf:Description crs:What="Correction"
crs:CorrectionName="Sky band" crs:CorrectionAmount="1" crs:CorrectionActive="true">
<crs:CorrectionMasks><rdf:Seq>
<rdf:li><rdf:Description crs:What="Mask/Image" crs:MaskActive="true" crs:MaskName="Sky 1"
crs:MaskBlendMode="0" crs:MaskInverted="false" crs:MaskValue="1" crs:MaskVersion="1"
crs:MaskSubType="2" crs:ReferencePoint="0.5 0.25"/></rdf:li>
<rdf:li crs:What="Mask/CircularGradient" crs:MaskActive="true" crs:MaskBlendMode="1"
crs:MaskInverted="true" crs:MaskValue="0" crs:Top="0.25" crs:Left="0.25" crs:Bottom="0.75"
crs:Right="0.75" crs:Feather="50" crs:Roundness="0" crs:Flipped="false" crs:Angle="0"/>
</rdf:Seq></crs:CorrectionMasks></rdf:Description></rdf:li></rdf:Seq></crs:MaskGroupBasedCorrections>
</rdf:Description>"#;

    let recipe = autoshade::xmp::xmp_to_recipe(xmp);

    assert_eq!(recipe.masks.len(), 1);
    assert!(matches!(
        recipe.masks[0].mask,
        MaskGeometry::AiMask { .. }
    ));
    assert_eq!(recipe.masks[0].components.len(), 1);
    assert_eq!(recipe.masks[0].components[0].mode, MaskCombine::Intersect);
    assert!(matches!(
        recipe.masks[0].components[0].geometry,
        MaskGeometry::Radial { .. }
    ));
}

#[test]
fn zone_roles_fall_back_from_measured_names_only_without_intent() {
    let read = |name: &str| {
        let xmp = format!(
            r#"<rdf:Description xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/">
<crs:MaskGroupBasedCorrections><rdf:Seq><rdf:li><rdf:Description crs:What="Correction"
crs:CorrectionName="{name}" crs:CorrectionAmount="1" crs:CorrectionActive="true">
<crs:CorrectionMasks><rdf:Seq><rdf:li><rdf:Description crs:What="Mask/Image"
crs:MaskActive="true" crs:MaskName="Sky 1" crs:MaskBlendMode="0"
crs:MaskInverted="false" crs:MaskValue="1" crs:MaskVersion="1"
crs:MaskSubType="2" crs:ReferencePoint="0.5 0.25"/></rdf:li></rdf:Seq>
</crs:CorrectionMasks></rdf:Description></rdf:li></rdf:Seq></crs:MaskGroupBasedCorrections>
</rdf:Description>"#
        );
        autoshade::xmp::xmp_to_recipe(&xmp).masks.remove(0)
    };

    let unnamed = read("sky");
    assert_eq!(unnamed.role, MaskRole::ZoneSky);
    assert!(unnamed.name.is_empty());
    let band = read("sky · band 2/3");
    assert_eq!(band.role, MaskRole::ZoneSky);
    assert_eq!(band.name, "sky · band 2/3");
    let grad = read("grad");
    assert_eq!(grad.role, MaskRole::Custom);
    assert_eq!(grad.name, "grad");
    let lightroom_sky = read("Sky 1");
    assert_eq!(lightroom_sky.role, MaskRole::Custom);
    assert_eq!(lightroom_sky.name, "Sky 1");
}

#[test]
fn explicit_role_intent_overrides_zone_name_fallback() {
    let mask = |name: &str, role| LocalAdjustment {
        name: name.to_string(),
        role,
        mask: MaskGeometry::select_sky(0.5, 0.25, false, String::new()),
        ..LocalAdjustment::default()
    };
    let xmp = without_payload(autoshade::xmp::recipe_to_xmp(&EditRecipe {
        masks: vec![
            mask("sky", MaskRole::Custom),
            mask("Sky 1", MaskRole::ZoneSky),
        ],
        ..EditRecipe::default()
    }));

    let restored = autoshade::xmp::xmp_to_recipe(&xmp);

    assert_eq!(restored.masks[0].role, MaskRole::Custom);
    assert_eq!(restored.masks[1].role, MaskRole::ZoneSky);
}

#[test]
fn an_untouched_external_rewrite_preserves_the_authored_inversion_home() {
    for geometry in [
        unbacked_sky_geometry(),
        MaskGeometry::Radial {
            top: 0.2,
            left: 0.2,
            bottom: 0.8,
            right: 0.8,
            feather: 0.5,
            roundness: 0.0,
            flipped: false,
            angle: 0.0,
            midpoint: 50.0,
            mask_version: 2,
        },
    ] {
        let original = LocalAdjustment {
            name: "authored inversion".to_string(),
            mask: geometry,
            inverted: true,
            exposure_ev: -0.5,
            ..LocalAdjustment::default()
        };
        let xmp = without_mask_intent(autoshade::xmp::recipe_to_xmp(&EditRecipe {
            masks: vec![original.clone()],
            ..EditRecipe::default()
        }));

        let restored = autoshade::xmp::xmp_to_recipe(&xmp);

        assert_eq!(restored.masks.len(), 1);
        assert_eq!(
            (
                restored.masks[0].inverted,
                restored.masks[0].mask.own_inverted(),
            ),
            (true, false)
        );
        assert_eq!(restored.masks[0].exposure_ev, original.exposure_ev);
    }
}

#[test]
fn an_external_inversion_edit_overrides_the_payload_in_the_native_home() {
    let geometries = [
        unbacked_sky_geometry(),
        MaskGeometry::Radial {
            top: 0.2,
            left: 0.2,
            bottom: 0.8,
            right: 0.8,
            feather: 0.5,
            roundness: 0.0,
            flipped: false,
            angle: 0.0,
            midpoint: 50.0,
            mask_version: 2,
        },
    ];
    for (index, geometry) in geometries.into_iter().enumerate() {
        let document = |inverted| {
            without_mask_intent(autoshade::xmp::recipe_to_xmp(&EditRecipe {
                masks: vec![LocalAdjustment {
                    name: "edited inversion".to_string(),
                    mask: geometry.clone(),
                    inverted,
                    exposure_ev: -0.5,
                    ..LocalAdjustment::default()
                }],
                ..EditRecipe::default()
            }))
        };

        let removed = autoshade::xmp::xmp_to_recipe(&replace_single_native_inversion(
            &document(true),
            true,
            false,
        ));
        assert!(!removed.masks[0].net_inverted());
        assert_eq!(removed.masks[0].exposure_ev, -0.5);

        let added = autoshade::xmp::xmp_to_recipe(&replace_single_native_inversion(
            &document(false),
            false,
            true,
        ));
        let expected_home = if index == 0 {
            (false, true)
        } else {
            (true, false)
        };
        assert_eq!(
            (added.masks[0].inverted, added.masks[0].mask.own_inverted()),
            expected_home
        );
        assert_eq!(added.masks[0].exposure_ev, -0.5);
    }
}

#[test]
fn xmp_payload_carries_additional_rasters_by_filename_only() {
    use base64::Engine as _;
    use std::io::Read as _;

    let tree = support::TempTree::new("xmp-additional-raster-paths");
    let source = tree.path().join("private-source-directory");
    std::fs::create_dir(&source).unwrap();
    for name in ["depth.png", "protect.png", "layer.png"] {
        std::fs::write(source.join(name), name.as_bytes()).unwrap();
    }
    let recipe = EditRecipe {
        lens_blur: Some(LensBlur {
            depth_map: source.join("depth.png").to_string_lossy().into_owned(),
            protect: Some(
                source
                    .join("protect.png")
                    .to_string_lossy()
                    .into_owned(),
            ),
            ..LensBlur::default()
        }),
        pixel_layers: vec![PixelLayer {
            path: source.join("layer.png").to_string_lossy().into_owned(),
            ..PixelLayer::default()
        }],
        ..EditRecipe::default()
    };

    let xmp = autoshade::xmp::recipe_to_xmp(&recipe);
    let packed = base64::engine::general_purpose::STANDARD
        .decode(xmp_attribute_values(&xmp, "asr:Recipe")[0])
        .unwrap();
    let mut json = Vec::new();
    flate2::read::ZlibDecoder::new(packed.as_slice())
        .read_to_end(&mut json)
        .unwrap();
    let carried: EditRecipe = serde_json::from_slice(&json).unwrap();
    let blur = carried.lens_blur.unwrap();

    assert_eq!(
        (
            blur.depth_map.as_str(),
            blur.protect.as_deref(),
            carried.pixel_layers[0].path.as_str(),
        ),
        ("depth.png", Some("protect.png"), "layer.png")
    );
}

fn merged_base_recipe() -> EditRecipe {
    EditRecipe {
        exposure_ev: 0.35,
        contrast: 12.0,
        saturation: -8.0,
        lens_profile: LensProfile {
            vignette_on: true,
            distortion_on: true,
            ca_on: true,
            ..LensProfile::default()
        },
        ..EditRecipe::default()
    }
}

#[test]
fn a_merged_lightroom_base_reopens_as_the_develop_autoshade_wrote() {
    let recipe = merged_base_recipe();

    let merged = autoshade::xmp::merge_recipe_into_xmp(LIGHTROOM_BASE, &recipe)
        .unwrap()
        .doc;
    let collector = autoshade::diag::Collector::new();
    let diag = autoshade::diag::Diag::pixels_only(&collector);
    let restored = autoshade::xmp::xmp_to_recipe_with_diag(&merged, &diag);

    assert!(merged.contains(r#"crs:CameraProfile="Adobe Standard""#));
    assert!(merged.contains("<crs:Look>"));
    assert!(merged.contains(r#"crs:LensProfileEnable="0""#));
    assert!(collector.is_empty());
    assert_eq!(restored, recipe);
    assert_eq!(
        autoshade::xmp::written_lens_profile(&merged),
        Some(recipe.lens_profile)
    );
}

#[test]
fn external_profile_and_lens_switch_edits_override_the_written_payload() {
    let recipe = merged_base_recipe();
    let merged = autoshade::xmp::merge_recipe_into_xmp(LIGHTROOM_BASE, &recipe)
        .unwrap()
        .doc;
    let profile_edit = merged.replacen(
        r#"crs:CameraProfile="Adobe Standard""#,
        r#"crs:CameraProfile="Adobe Portrait""#,
        1,
    );
    let lens_edit = merged.replacen(
        r#"crs:LensProfileEnable="0""#,
        r#"crs:LensProfileEnable="1""#,
        1,
    );

    assert_eq!(
        autoshade::xmp::xmp_to_recipe(&profile_edit).camera_profile,
        "Adobe Portrait"
    );
    assert_eq!(autoshade::xmp::written_lens_profile(&lens_edit), None);
}

#[test]
fn a_corrupt_written_record_is_disclosed_while_the_payload_recipe_still_opens() {
    let recipe = merged_base_recipe();
    let merged = autoshade::xmp::merge_recipe_into_xmp(LIGHTROOM_BASE, &recipe)
        .unwrap()
        .doc;
    let key = "asr:WrittenCrc32=\"";
    let start = merged.find(key).unwrap() + key.len();
    let mut tampered = merged.clone();
    let checksum = start..start + 8;
    let replacement = if &merged[checksum.clone()] == "00000000" {
        "00000001"
    } else {
        "00000000"
    };
    tampered.replace_range(checksum, replacement);
    let collector = autoshade::diag::Collector::new();
    let diag = autoshade::diag::Diag::pixels_only(&collector);

    let restored = autoshade::xmp::xmp_to_recipe_with_diag(&tampered, &diag);
    let lines = collector.take();

    assert_eq!(restored.exposure_ev, recipe.exposure_ev);
    assert!(lines.iter().any(|line| {
        line.text.contains("record of the document AutoShade wrote")
            && line.text.contains("checksum")
    }));
}

#[test]
fn an_unsupported_payload_format_is_disclosed_while_the_crs_projection_opens() {
    let original = autoshade::xmp::recipe_to_xmp(&EditRecipe {
        exposure_ev: 0.5,
        ..EditRecipe::default()
    });
    let format = xmp_attribute_values(&original, "asr:Payload")[0];
    let unsupported = original.replacen(
        &format!("asr:Payload=\"{format}\""),
        "asr:Payload=\"future-format\"",
        1,
    );
    let collector = autoshade::diag::Collector::new();
    let diag = autoshade::diag::Diag::pixels_only(&collector);

    let restored = autoshade::xmp::xmp_to_recipe_with_diag(&unsupported, &diag);
    let lines = collector.take();

    assert_eq!(restored.exposure_ev, 0.5);
    assert!(lines.iter().any(|line| line.text.contains("payload format")));
}

fn lightroom_payload_fixture(name: &str) -> String {
    let root = std::env::var_os("AUTOSHADE_LR_PAYLOAD_FIXTURES")
        .map(std::path::PathBuf::from)
        .expect("set AUTOSHADE_LR_PAYLOAD_FIXTURES to the lr-payload-2026-09 fixture directory");
    std::fs::read_to_string(root.join(name))
        .unwrap_or_else(|error| panic!("{name}: {error}"))
}

#[test]
#[ignore = "requires the external lr-payload-2026-09 Lightroom rewrite fixtures"]
fn real_lightroom_probe_preserves_payload_and_raster_bytes() {
    let original = lightroom_payload_fixture("probe-original.xmp");
    let rewritten = lightroom_payload_fixture("probe-lightroom-9.4-rewrite.xmp");
    let original_recipe = xmp_attribute_values(&original, "asr:Recipe");
    let rewritten_recipe = xmp_attribute_values(&rewritten, "asr:Recipe");

    assert_ne!(original, rewritten);
    assert_eq!(original_recipe.len(), 1);
    assert!(original_recipe[0].len() > 8_000);
    assert_eq!(original_recipe, rewritten_recipe);
    assert_eq!(
        xmp_attribute_values(&original, "asr:RecipeSha256"),
        xmp_attribute_values(&rewritten, "asr:RecipeSha256")
    );
    for field in ["Name", "Sha256", "Data"] {
        assert_eq!(
            xmp_attribute_values(&original, &format!("asr:{field}")),
            xmp_attribute_values(&rewritten, &format!("asr:{field}"))
        );
    }
    let raster_data = xmp_attribute_values(&rewritten, "asr:Data");
    assert_eq!(raster_data.len(), 5);
    assert!(raster_data.iter().all(|data| data.len() > 1_000));

    let collector = autoshade::diag::Collector::new();
    let diag = autoshade::diag::Diag::pixels_only(&collector);
    let _ = autoshade::xmp::xmp_to_recipe_with_diag(&rewritten, &diag);
    assert!(collector
        .take()
        .iter()
        .any(|line| line.text.contains("payload format")));
}

#[test]
#[ignore = "requires the external lr-payload-2026-09 Lightroom rewrite fixtures"]
fn real_lightroom_v130_rewrite_recovers_zone_roles_from_band_names() {
    let original = lightroom_payload_fixture("band-sidecar-original.xmp");
    let rewritten = lightroom_payload_fixture("band-sidecar-lightroom-9.4-rewrite.xmp");

    assert!(original.contains("ash:Role=\"sky\""));
    assert!(!rewritten.contains("ash:"));
    let before = autoshade::xmp::xmp_to_recipe(&original);
    let after = autoshade::xmp::xmp_to_recipe(&rewritten);

    assert_eq!(before.masks.len(), 2);
    assert_eq!(after.masks.len(), 2);
    for (original_mask, rewritten_mask) in before.masks.iter().zip(&after.masks) {
        assert_eq!(original_mask.role, MaskRole::ZoneSky);
        assert_eq!(rewritten_mask.role, MaskRole::ZoneSky);
        assert_eq!(rewritten_mask.name, original_mask.name);
        assert!(rewritten_mask.name.starts_with("sky · band "));
    }
    assert_eq!(before.color_nr, 0.0);
    assert_eq!(after.color_nr, 25.0);
}

#[test]
#[ignore = "requires the external lr-payload-2026-09 Lightroom rewrite fixtures"]
fn real_lightroom_v131_rewrite_restores_the_payload_develop() {
    let original = lightroom_payload_fixture("payload-131-original.xmp");
    let rewritten = lightroom_payload_fixture("payload-131-lightroom-9.4-rewrite.xmp");

    assert_ne!(original, rewritten);
    assert!(original.contains("asr:Writer=\"AutoShade 1.3.1\""));
    assert!(!rewritten.contains(" ash:"));
    assert_eq!(
        xmp_attribute_values(&original, "asr:Recipe"),
        xmp_attribute_values(&rewritten, "asr:Recipe")
    );
    for field in ["Name", "Data"] {
        assert_eq!(
            xmp_attribute_values(&original, &format!("asr:{field}")),
            xmp_attribute_values(&rewritten, &format!("asr:{field}"))
        );
    }
    assert_eq!(xmp_attribute_values(&rewritten, "asr:Data").len(), 5);

    let collector = autoshade::diag::Collector::new();
    let diag = autoshade::diag::Diag::pixels_only(&collector);
    let before = autoshade::xmp::xmp_to_recipe(&original);
    let after = autoshade::xmp::xmp_to_recipe_with_diag(&rewritten, &diag);

    assert!(collector.is_empty());
    assert_eq!(after.colour_field, before.colour_field);
    assert_eq!(after.masks.len(), 6);
    for (rewritten_mask, original_mask) in after.masks.iter().zip(&before.masks) {
        assert_eq!(
            (
                &rewritten_mask.name,
                rewritten_mask.role,
                rewritten_mask.enabled,
                rewritten_mask.inverted,
            ),
            (
                &original_mask.name,
                original_mask.role,
                original_mask.enabled,
                original_mask.inverted,
            )
        );
    }
    assert_eq!(
        after
            .masks
            .iter()
            .filter(|mask| mask.role == MaskRole::ZoneSky)
            .count(),
        2
    );
    assert_eq!(
        (after.exposure_ev, after.temperature_k, after.as_shot_k),
        (before.exposure_ev, before.temperature_k, before.as_shot_k)
    );
    assert_eq!(after.color_nr, 0.0);
}

#[test]
fn an_external_rewrite_keeps_untouched_payload_state_and_applies_its_edits() {
    let zone = LocalAdjustment {
        mask: unbacked_sky_geometry(),
        role: MaskRole::ZoneSky,
        components: vec![MaskComponent {
            geometry: MaskGeometry::Radial {
                top: 0.3,
                left: 0.3,
                bottom: 0.7,
                right: 0.7,
                feather: 0.5,
                roundness: 0.0,
                flipped: false,
                angle: 0.0,
                midpoint: 50.0,
                mask_version: 2,
            },
            mode: MaskCombine::Intersect,
            inverted: false,
        }],
        exposure_ev: -0.25,
        ..LocalAdjustment::default()
    };
    let spot = LocalAdjustment {
        name: "spot".to_string(),
        mask: MaskGeometry::Radial {
            top: 0.2,
            left: 0.2,
            bottom: 0.4,
            right: 0.4,
            feather: 0.5,
            roundness: 0.0,
            flipped: false,
            angle: 0.0,
            midpoint: 50.0,
            mask_version: 2,
        },
        exposure_ev: 0.25,
        ..LocalAdjustment::default()
    };
    let grad = LocalAdjustment {
        name: "grad".to_string(),
        mask: MaskGeometry::Linear {
            zero_x: 0.1,
            zero_y: 0.25,
            full_x: 0.9,
            full_y: 0.75,
        },
        amount: 1.0,
        contrast: 20.0,
        ..LocalAdjustment::default()
    };
    let original = EditRecipe {
        exposure_ev: 0.35,
        contrast: 12.0,
        temperature_k: Some(5600.0),
        tint: 3.0,
        as_shot_k: Some(5200.0),
        color_nr_detail: 0.0,
        color_nr_smooth: 0.0,
        colour_field: Some(ColourField {
            x: 1,
            y: 1,
            b: 1,
            grid: vec![[0.1, 0.0, 0.0, 0.0, 0.0]],
            amount: 0.8,
            enabled: true,
        }),
        masks: vec![zone.clone(), spot, grad.clone()],
        rationale: "measured payload state".to_string(),
        confidence: 0.8,
        ..EditRecipe::default()
    };
    let mut edited = original.clone();
    edited.exposure_ev = -0.5;
    edited.color_nr_detail = 50.0;
    edited.color_nr_smooth = 50.0;
    edited.masks.retain(|mask| mask.name != "spot");
    edited.masks[1].amount = 0.4;
    edited.masks.push(LocalAdjustment {
        name: "LR added".to_string(),
        mask: MaskGeometry::Linear {
            zero_x: 0.25,
            zero_y: 0.25,
            full_x: 0.75,
            full_y: 0.75,
        },
        exposure_ev: -1.0,
        ..LocalAdjustment::default()
    });

    let xmp = external_rewrite(&original, &edited);
    let restored = autoshade::xmp::xmp_to_recipe(&xmp);
    let names = restored
        .masks
        .iter()
        .map(|mask| mask.name.as_str())
        .collect::<Vec<_>>();

    assert!(!xmp.contains("ash:"));
    assert!(xmp.contains("crs:ColorNoiseReductionDetail=\"50\""));
    assert_eq!(restored.exposure_ev, -0.5);
    assert_eq!(names, ["", "grad", "LR added"]);
    assert_eq!(restored.masks[0], zone);
    assert_eq!(restored.masks[1].amount, 0.4);
    assert_eq!(restored.masks[1].contrast, 20.0);
    assert_eq!(restored.masks[1].mask, grad.mask);
    assert_eq!(restored.masks[2].role, MaskRole::Custom);
    assert_eq!(restored.masks[2].mask, edited.masks[2].mask);
    assert!((restored.masks[2].exposure_ev + 1.0).abs() < 1e-3);
    assert_eq!(restored.masks[0].role, MaskRole::ZoneSky);
    assert_eq!(
        restored.masks[0].components[0].mode,
        MaskCombine::Intersect
    );
    assert_eq!(restored.colour_field, original.colour_field);
    assert_eq!(restored.contrast, 12.0);
    assert_eq!(restored.tint, 3.0);
    assert_eq!(restored.temperature_k, Some(5600.0));
    assert_eq!(restored.as_shot_k, Some(5200.0));
    assert_eq!(restored.rationale, "measured payload state");
    assert_eq!(restored.confidence, 0.8);
    assert_eq!(restored.color_nr_detail, 0.0);
    assert_eq!(restored.color_nr_smooth, 0.0);
}

#[test]
fn an_external_recolour_edit_replaces_the_payload_gains() {
    let original = EditRecipe {
        masks: vec![LocalAdjustment {
            name: "spot".to_string(),
            mask: MaskGeometry::Radial {
                top: 0.2,
                left: 0.2,
                bottom: 0.4,
                right: 0.4,
                feather: 0.5,
                roundness: 0.0,
                flipped: false,
                angle: 0.0,
                midpoint: 50.0,
                mask_version: 2,
            },
            exposure_ev: 0.25,
            color_gains: Some([1.18, 0.96, 0.85]),
            ..LocalAdjustment::default()
        }],
        ..EditRecipe::default()
    };
    let mut edited = original.clone();
    edited.masks[0].color_gains = None;
    edited.masks[0].temperature = 50.0;

    let restored =
        autoshade::xmp::xmp_to_recipe(&external_rewrite(&original, &edited));
    let spot = &restored.masks[0];

    assert_eq!(spot.color_gains, None);
    assert!((spot.temperature - 50.0).abs() < 1e-3);
    assert!((spot.exposure_ev - 0.25).abs() < 1e-3);
}

#[test]
fn an_explicit_companion_zero_survives_until_an_external_edit_replaces_it() {
    let mut original = EditRecipe {
        sharpening: 40.0,
        ..EditRecipe::default()
    };
    original.set_resolved("sharpen_detail", 0.0);
    let original_xmp = autoshade::xmp::recipe_to_xmp(&original);
    assert!(original_xmp.contains("crs:SharpenDetail=\"0\""));

    let untouched =
        autoshade::xmp::xmp_to_recipe(&external_rewrite(&original, &original));
    assert_eq!(
        untouched.explicit_zero,
        vec!["sharpen_detail".to_string()]
    );
    assert_eq!(untouched.resolved("sharpen_detail"), 0.0);

    let mut edited = original.clone();
    edited.set_resolved("sharpen_detail", 40.0);
    let replaced =
        autoshade::xmp::xmp_to_recipe(&external_rewrite(&original, &edited));
    assert!(replaced.explicit_zero.is_empty());
    assert_eq!(replaced.resolved("sharpen_detail"), 40.0);
}

#[test]
fn disclosing_xmp_reads_place_rasters_without_overwriting_existing_files() {
    let tree = support::TempTree::new("xmp-raster-placement");
    let source = tree.path().join("source");
    std::fs::create_dir(&source).unwrap();
    let alpha = source.join("mask-zone-sky.png");
    let tile = source.join("tile-r3c3.png");
    std::fs::write(&alpha, b"zone alpha").unwrap();
    std::fs::write(&tile, b"tile bytes").unwrap();
    let recipe = EditRecipe {
        masks: vec![
            LocalAdjustment {
                mask: MaskGeometry::Bitmap {
                    path: alpha.to_string_lossy().into_owned(),
                },
                ..LocalAdjustment::default()
            },
            LocalAdjustment {
                mask: MaskGeometry::Bitmap {
                    path: tile.to_string_lossy().into_owned(),
                },
                ..LocalAdjustment::default()
            },
        ],
        ..EditRecipe::default()
    };
    let xmp = autoshade::xmp::recipe_to_xmp(&recipe);
    let mut env = support::ScopedEnv::new();
    env.set("AUTOSHADE_DATA_DIR", tree.path().join("store"));
    let photo = tree.path().join("target.dng");
    std::fs::write(&photo, b"synthetic photo").unwrap();
    let develop = autoshade::store::develop_dir(&photo);

    let silent = autoshade::xmp::xmp_to_recipe_for_photo(&xmp, &photo);
    assert!(!develop.join("mask-zone-sky.png").exists());
    assert!(!develop.join("tile-r3c3.png").exists());
    for mask in &silent.masks {
        let MaskGeometry::Bitmap { path } = &mask.mask else {
            panic!("payload bitmap remains a bitmap")
        };
        assert_eq!(
            std::path::Path::new(path),
            develop.join(".invalid-mask-reference")
        );
    }

    let collector = autoshade::diag::Collector::new();
    let diag = autoshade::diag::Diag::about(&collector, &photo);
    let placed = autoshade::xmp::xmp_to_recipe_with_diag(&xmp, &diag);
    assert_eq!(std::fs::read(develop.join("mask-zone-sky.png")).unwrap(), b"zone alpha");
    assert_eq!(std::fs::read(develop.join("tile-r3c3.png")).unwrap(), b"tile bytes");
    let MaskGeometry::Bitmap { path } = &placed.masks[1].mask else {
        panic!("placed tile remains a bitmap")
    };
    assert_eq!(std::path::Path::new(path), develop.join("tile-r3c3.png"));
    assert!(collector.take().is_empty());

    let _ = autoshade::xmp::xmp_to_recipe_with_diag(&xmp, &diag);
    assert!(collector.take().is_empty());

    std::fs::write(develop.join("tile-r3c3.png"), b"someone else's tile").unwrap();
    let moved = autoshade::xmp::xmp_to_recipe_with_diag(&xmp, &diag);
    let MaskGeometry::Bitmap { path } = &moved.masks[1].mask else {
        panic!("moved tile remains a bitmap")
    };
    assert_eq!(std::path::Path::new(path), develop.join("tile-r3c3-2.png"));
    assert_eq!(std::fs::read(develop.join("tile-r3c3-2.png")).unwrap(), b"tile bytes");
    assert_eq!(
        std::fs::read(develop.join("tile-r3c3.png")).unwrap(),
        b"someone else's tile"
    );
    let lines = collector.take();
    assert_eq!(lines.len(), 1);
    assert!(lines[0].text.contains("tile-r3c3-2.png"));
}

#[cfg(unix)]
#[test]
#[allow(clippy::disallowed_methods)]
fn a_failed_raster_claim_releases_the_half_written_name() {
    let tree = support::TempTree::new("xmp-raster-claim-failure");
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .arg("--ignored")
        .arg("--exact")
        .arg("raster_claim_write_failure_fixture")
        .env("AUTOSHADE_RASTER_CLAIM_FAILURE_ROOT", tree.path())
        .env("AUTOSHADE_DATA_DIR", tree.path().join("store"))
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "fixture failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(unix)]
#[test]
#[ignore = "subprocess fixture invoked by a_failed_raster_claim_releases_the_half_written_name"]
fn raster_claim_write_failure_fixture() {
    let root = std::env::var_os("AUTOSHADE_RASTER_CLAIM_FAILURE_ROOT")
        .map(std::path::PathBuf::from)
        .expect("fixture must be launched by its parent case");
    let source = root.join("source");
    std::fs::create_dir_all(&source).unwrap();
    let tile = source.join("tile-r3c3.png");
    std::fs::write(&tile, b"payload tile").unwrap();
    let xmp = autoshade::xmp::recipe_to_xmp(&EditRecipe {
        masks: vec![LocalAdjustment {
            mask: MaskGeometry::Bitmap {
                path: tile.to_string_lossy().into_owned(),
            },
            ..LocalAdjustment::default()
        }],
        ..EditRecipe::default()
    });
    let photo = root.join("target.dng");
    std::fs::write(&photo, b"synthetic photo").unwrap();
    let develop = autoshade::store::develop_dir(&photo);
    std::fs::create_dir_all(&develop).unwrap();
    std::fs::write(develop.join("tile-r3c3.png"), b"existing tile").unwrap();

    unsafe {
        assert_ne!(libc::signal(libc::SIGXFSZ, libc::SIG_IGN), libc::SIG_ERR);
        let limit = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        assert_eq!(libc::setrlimit(libc::RLIMIT_FSIZE, &limit), 0);
    }

    let collector = autoshade::diag::Collector::new();
    let diag = autoshade::diag::Diag::about(&collector, &photo);
    let _ = autoshade::xmp::xmp_to_recipe_with_diag(&xmp, &diag);
    let lines = collector.take();

    assert!(!develop.join("tile-r3c3-2.png").exists());
    assert_eq!(
        std::fs::read(develop.join("tile-r3c3.png")).unwrap(),
        b"existing tile"
    );
    assert_eq!(lines.len(), 1);
    assert!(lines[0].text.contains("could not be placed"));
}

#[test]
fn a_photo_less_xmp_read_disables_every_unanchored_raster_reference() {
    let bitmap = |path: &str| LocalAdjustment {
        mask: MaskGeometry::Bitmap {
            path: path.to_string(),
        },
        exposure_ev: 0.5,
        ..LocalAdjustment::default()
    };
    let recipe = EditRecipe {
        exposure_ev: 0.3,
        masks: vec![
            bitmap("mask-bare.png"),
            bitmap("sub/relative.png"),
            bitmap("//host/share/mask.png"),
        ],
        lens_blur: Some(LensBlur {
            depth_map: "../depth.png".to_string(),
            protect: Some(r"\\host\share\protect.png".to_string()),
            ..LensBlur::default()
        }),
        pixel_layers: vec![PixelLayer {
            path: "layer.png".to_string(),
            ..PixelLayer::default()
        }],
        ..EditRecipe::default()
    };

    let restored =
        autoshade::xmp::xmp_to_recipe(&autoshade::xmp::recipe_to_xmp(&recipe));
    let mut paths = restored;
    let paths = paths.raster_paths_mut();

    assert_eq!(paths.len(), 6);
    assert!(paths.iter().all(|path| {
        let path = std::path::Path::new(path.as_str());
        path.is_absolute() && path.ends_with(".invalid-mask-reference")
    }));
}

#[test]
fn a_photo_less_merge_preserves_an_untouched_bare_raster_import() {
    let recipe = EditRecipe {
        exposure_ev: 0.3,
        masks: vec![
            LocalAdjustment {
                mask: MaskGeometry::Radial {
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
                exposure_ev: 0.4,
                ..LocalAdjustment::default()
            },
            LocalAdjustment {
                mask: MaskGeometry::Bitmap {
                    path: "subject.png".to_string(),
                },
                exposure_ev: 0.6,
                ..LocalAdjustment::default()
            },
        ],
        ..EditRecipe::default()
    };
    let base = autoshade::xmp::recipe_to_xmp(&recipe);
    let imported = autoshade::xmp::xmp_to_recipe(&base);
    assert_eq!(imported.masks.len(), 2);

    let merged = autoshade::xmp::merge_recipe_into_xmp(&base, &imported).unwrap();

    assert!(!merged
        .notes
        .iter()
        .any(|note| note.contains("merge base's mask block")));
    assert_eq!(autoshade::xmp::xmp_to_recipe(&merged.doc).masks.len(), 2);
}

#[test]
fn an_oversized_payload_raster_is_left_out_and_named_as_a_loss() {
    let tree = support::TempTree::new("xmp-raster-budget");
    let huge = tree.path().join("huge.png");
    std::fs::write(&huge, vec![0x42; 7 * 1024 * 1024]).unwrap();
    let recipe = EditRecipe {
        masks: vec![LocalAdjustment {
            name: "huge".to_string(),
            mask: MaskGeometry::Bitmap {
                path: huge.to_string_lossy().into_owned(),
            },
            exposure_ev: 0.1,
            ..LocalAdjustment::default()
        }],
        ..EditRecipe::default()
    };

    let (xmp, losses) = autoshade::xmp::recipe_to_xmp_with_losses(&recipe);

    assert!(losses.iter().any(|loss| {
        loss.name == "huge"
            && loss.reason == autoshade::xmp::MaskLossReason::RasterNotEmbedded
    }));
    assert!(!xmp.contains("asr:Name=\"huge.png\""));
    assert!(xmp.len() < 2 * 1024 * 1024);
    let restored = autoshade::xmp::xmp_to_recipe(&xmp);
    assert_eq!(restored.masks.len(), 1);
    assert_eq!(restored.masks[0].name, "huge");
}

#[test]
fn a_shared_unembeddable_raster_is_a_loss_for_every_mask_that_names_it() {
    let tree = support::TempTree::new("xmp-shared-raster-loss");
    let missing = tree.path().join("missing.png");
    let bitmap = |name: &str| LocalAdjustment {
        name: name.to_string(),
        mask: MaskGeometry::Bitmap {
            path: missing.to_string_lossy().into_owned(),
        },
        ..LocalAdjustment::default()
    };
    let recipe = EditRecipe {
        masks: vec![bitmap("sky"), bitmap("sky · band 1/3")],
        ..EditRecipe::default()
    };

    let (xmp, losses) = autoshade::xmp::recipe_to_xmp_with_losses(&recipe);
    let lost = losses
        .iter()
        .filter(|loss| loss.reason == autoshade::xmp::MaskLossReason::RasterNotEmbedded)
        .map(|loss| loss.name.as_str())
        .collect::<Vec<_>>();

    assert_eq!(lost, ["sky", "sky · band 1/3"]);
    assert!(!xmp.contains("<asr:Rasters>"));
}

#[test]
fn an_inflated_recipe_over_the_ceiling_is_too_large_not_corrupt() {
    use base64::Engine as _;
    use std::io::Write as _;

    let original = autoshade::xmp::recipe_to_xmp(&EditRecipe {
        exposure_ev: 0.5,
        ..EditRecipe::default()
    });
    let oversized = vec![b' '; 17 * 1024 * 1024];
    let mut encoder =
        flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
    encoder.write_all(&oversized).unwrap();
    let packed = base64::engine::general_purpose::STANDARD
        .encode(encoder.finish().unwrap());
    let recipe = xmp_attribute_values(&original, "asr:Recipe")[0];
    let crc = xmp_attribute_values(&original, "asr:RecipeCrc32")[0];
    let tampered = original
        .replacen(
            &format!("asr:Recipe=\"{recipe}\""),
            &format!("asr:Recipe=\"{packed}\""),
            1,
        )
        .replacen(
            &format!("asr:RecipeCrc32=\"{crc}\""),
            &format!("asr:RecipeCrc32=\"{:08x}\"", crc32fast::hash(&oversized)),
            1,
        );
    let collector = autoshade::diag::Collector::new();
    let diag = autoshade::diag::Diag::pixels_only(&collector);

    let restored = autoshade::xmp::xmp_to_recipe_with_diag(&tampered, &diag);
    let lines = collector.take();
    let disclosure = lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(disclosure.contains("too large"), "{disclosure}");
    assert!(!disclosure.contains("checksum"), "{disclosure}");
    assert_eq!(restored.exposure_ev, 0.5);
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
