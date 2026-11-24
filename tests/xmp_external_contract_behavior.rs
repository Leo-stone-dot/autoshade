use autoshade::recipe::{
    AdobeLensProfile, ColorGrade, Crop, CurvePoint, EditRecipe, Hsl, LensProfile,
    LocalAdjustment, MaskCombine, MaskComponent, MaskGeometry, RangeMask,
};
use autoshade::xmp::{LensProfileSetup, MaskImportReason, MaskLossReason};

mod support;

fn without_autoshade_mask_intent(mut xmp: String) -> String {
    while let Some(start) = xmp.find(" ash:") {
        let value = start + xmp[start..].find("=\"").unwrap() + 2;
        let end = value + xmp[value..].find('"').unwrap() + 1;
        xmp.replace_range(start..end, "");
    }
    xmp.replace(" xmlns:ash=\"https://autoshade.dev/ns/mask/1.0/\"", "")
}

fn without_recipe_payload(mut xmp: String) -> String {
    for name in ["asr:Recipe", "asr:RecipeCrc32"] {
        let marker = format!(" {name}=\"");
        while let Some(start) = xmp.find(&marker) {
            let value = start + marker.len();
            let end = value + xmp[value..].find('"').unwrap() + 1;
            xmp.replace_range(start..end, "");
        }
    }
    xmp
}

#[test]
fn global_develop_controls_round_trip_through_our_xmp() {
    let recipe = EditRecipe {
        exposure_ev: 0.32,
        contrast: 14.0,
        highlights: -12.0,
        shadows: 25.0,
        whites: 8.0,
        blacks: -6.0,
        temperature_k: Some(5600.0),
        tint: 3.0,
        vibrance: 18.0,
        saturation: -5.0,
        clarity: 10.0,
        dehaze: 7.0,
        hsl: Hsl {
            hue: [0.0, 15.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            saturation: [0.0, 0.0, 0.0, -40.0, 0.0, 0.0, 0.0, 0.0],
            ..Hsl::default()
        },
        color_grade: ColorGrade {
            shadow_hue: 220.0,
            shadow_sat: 30.0,
            highlight_hue: 45.0,
            highlight_sat: 20.0,
            midtone_lum: -10.0,
            balance: 15.0,
            ..ColorGrade::default()
        },
        sharpening: 45.0,
        noise_reduction: 20.0,
        lens_vignette: 35.0,
        lens_vignette_mid: 60.0,
        lens_distortion: -24.0,
        straighten_deg: 1.5,
        crop: Some(Crop {
            left: 0.05,
            top: 0.0,
            right: 0.95,
            bottom: 1.0,
        }),
        tone_curve: vec![
            CurvePoint {
                input: 0,
                output: 8,
            },
            CurvePoint {
                input: 255,
                output: 247,
            },
        ],
        red_curve: vec![
            CurvePoint {
                input: 0,
                output: 10,
            },
            CurvePoint {
                input: 255,
                output: 250,
            },
        ],
        rationale: "global round trip".to_string(),
        confidence: 0.82,
        ..EditRecipe::default()
    };

    let restored =
        autoshade::xmp::xmp_to_recipe(&autoshade::xmp::recipe_to_xmp(&recipe));

    assert_eq!(restored, recipe);
}

#[test]
fn local_develop_controls_round_trip_through_our_xmp() {
    let recipe = EditRecipe {
        masks: vec![LocalAdjustment {
            mask: MaskGeometry::Linear {
                zero_x: 0.5,
                zero_y: 0.35,
                full_x: 0.5,
                full_y: 0.0,
            },
            range: Some(RangeMask::Luminance {
                lo_outer: 0.4,
                lo: 0.5,
                hi: 1.0,
                hi_outer: 1.0,
            }),
            name: "sky & sea".to_string(),
            amount: 0.75,
            inverted: true,
            exposure_ev: -0.4,
            contrast: 30.0,
            highlights: -50.0,
            shadows: 60.0,
            whites: 10.0,
            blacks: -20.0,
            clarity: 40.0,
            dehaze: 5.0,
            texture: 15.0,
            sharpness: -45.0,
            saturation: 20.0,
            hue: 35.0,
            temperature: 25.0,
            tint: -10.0,
            noise_reduction: 30.0,
            ..LocalAdjustment::default()
        }],
        ..EditRecipe::default()
    };

    let restored =
        autoshade::xmp::xmp_to_recipe(&autoshade::xmp::recipe_to_xmp(&recipe));

    assert_eq!(restored, recipe);
}

#[test]
fn foreign_as_shot_wb_is_neutral_but_custom_wb_imports() {
    let as_shot = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/" x:xmptk="Adobe XMP Core">
<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
<rdf:Description xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"
 crs:WhiteBalance="As Shot" crs:Temperature="5150" crs:Tint="+10"
 crs:Exposure2012="+0.65" crs:Contrast2012="+22" crs:Sharpness="40">
 <crs:ToneCurvePV2012><rdf:Seq>
  <rdf:li>0, 0</rdf:li><rdf:li>255, 255</rdf:li>
 </rdf:Seq></crs:ToneCurvePV2012>
</rdf:Description></rdf:RDF></x:xmpmeta>"#;

    let neutral = autoshade::xmp::xmp_to_recipe(as_shot);
    let custom = autoshade::xmp::xmp_to_recipe(&as_shot.replace("As Shot", "Custom"));

    assert_eq!((neutral.temperature_k, neutral.tint), (None, 0.0));
    assert_eq!((neutral.exposure_ev, neutral.contrast), (0.65, 22.0));
    assert_eq!(neutral.sharpening, 40.0);
    assert!(neutral.tone_curve.is_empty());
    assert_eq!((custom.temperature_k, custom.tint), (Some(5150.0), 10.0));
}

#[test]
fn hostile_xml_text_round_trips_once_without_changing_user_values() {
    let hostile = r#"& < > " ' literal &lt; masks\Bob's "sky".xmp"#;
    let recipe = EditRecipe {
        rationale: hostile.to_string(),
        masks: vec![LocalAdjustment {
            name: hostile.to_string(),
            ..LocalAdjustment::default()
        }],
        ..EditRecipe::default()
    };

    let xmp = autoshade::xmp::recipe_to_xmp(&recipe);
    let restored = autoshade::xmp::xmp_to_recipe(&xmp);

    assert!(xmp.contains("&quot;sky&quot;"));
    assert_eq!(restored.rationale, hostile);
    assert_eq!(restored.masks[0].name, hostile);
}

#[test]
fn comments_and_attribute_whitespace_cannot_hijack_the_crs_description() {
    let fake = r#"<!-- <rdf:Description xmlns:crs="urn:fake" crs:Exposure2012="9"/> -->"#;
    let document = format!(
        r#"{fake}
<rdf:Description xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"
 crs:Exposure2012 = "+0.65"></rdf:Description>"#
    );

    let imported = autoshade::xmp::xmp_to_recipe(&document);
    let merged = autoshade::xmp::merge_recipe_into_xmp(
        &document,
        &EditRecipe {
            exposure_ev: 0.25,
            ..EditRecipe::default()
        },
    )
    .unwrap();

    assert_eq!(imported.exposure_ev, 0.65);
    assert!(merged.doc.contains(fake));
    assert_eq!(autoshade::xmp::xmp_to_recipe(&merged.doc).exposure_ev, 0.25);
}

#[test]
fn attribute_form_curve_is_read_and_replaced_exactly_once() {
    let document = r#"<rdf:Description
 xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/">
 <crs:ToneCurvePV2012 xml:lang="x-default"><rdf:Seq>
  <rdf:li>0, 20</rdf:li><rdf:li>255, 240</rdf:li>
 </rdf:Seq></crs:ToneCurvePV2012>
</rdf:Description>"#;

    let imported = autoshade::xmp::xmp_to_recipe(document);
    let merged = autoshade::xmp::merge_recipe_into_xmp(
        document,
        &EditRecipe {
            tone_curve: vec![
                autoshade::recipe::CurvePoint {
                    input: 0,
                    output: 5,
                },
                autoshade::recipe::CurvePoint {
                    input: 255,
                    output: 250,
                },
            ],
            ..EditRecipe::default()
        },
    )
    .unwrap();

    assert_eq!(imported.tone_curve[0].output, 20);
    assert!(!merged.doc.contains("0, 20"));
    assert_eq!(
        autoshade::xmp::xmp_to_recipe(&merged.doc).tone_curve[0].output,
        5
    );
    assert_eq!(merged.doc.matches("<crs:ToneCurvePV2012>").count(), 1);
}

#[test]
fn malformed_attribute_form_curve_is_disclosed_and_not_partially_imported() {
    let out_of_domain = r#"<rdf:Description
 xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/">
 <crs:ToneCurvePV2012 xml:lang="x-default"><rdf:Seq>
  <rdf:li>999, -5</rdf:li>
 </rdf:Seq></crs:ToneCurvePV2012>
</rdf:Description>"#;
    let unterminated = r#"<rdf:Description
 xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/">
 <crs:ToneCurvePV2012><rdf:Seq><rdf:li>0, 0</rdf:li></rdf:Seq>
</rdf:Description>"#;

    for document in [out_of_domain, unterminated] {
        assert!(autoshade::xmp::xmp_to_recipe(document).tone_curve.is_empty());
        assert!(autoshade::xmp::unparsable_crs_numbers(document)
            .iter()
            .any(|key| key == "ToneCurvePV2012"));
    }
}

#[test]
fn trailing_space_in_a_close_tag_still_ends_the_property() {
    let document = r#"<rdf:Description
 xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/">
 <crs:Exposure2012>+0.65</crs:Exposure2012 >
</rdf:Description>"#;

    assert_eq!(autoshade::xmp::xmp_to_recipe(document).exposure_ev, 0.65);
}

fn foreign_brush_mask_block() -> &'static str {
    r#"<x:xmpmeta xmlns:x="adobe:ns:meta/">
<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
<rdf:Description xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/">
 <crs:MaskGroupBasedCorrections rdf:parseType="Resource"><rdf:Seq>
  <rdf:li><rdf:Description crs:What="Correction" crs:CorrectionActive="true">
   <crs:CorrectionMasks><rdf:Seq>
    <rdf:li crs:What="Mask/Brush"/>
   </rdf:Seq></crs:CorrectionMasks>
  </rdf:Description></rdf:li>
 </rdf:Seq></crs:MaskGroupBasedCorrections>
</rdf:Description></rdf:RDF></x:xmpmeta>"#
}

#[test]
fn foreign_mask_block_is_preserved_until_the_recipe_replaces_it_with_disclosure() {
    let existing = foreign_brush_mask_block();

    assert_eq!(autoshade::xmp::unsupported_corrections(existing), 1);
    let preserved = autoshade::xmp::merge_recipe_into_xmp(
        existing,
        &EditRecipe {
            exposure_ev: 0.25,
            ..EditRecipe::default()
        },
    )
    .unwrap();
    assert!(preserved.doc.contains("Mask/Brush"));
    assert!(preserved.notes.is_empty());

    let replacement = EditRecipe {
        masks: vec![LocalAdjustment {
            name: "face".to_string(),
            mask: MaskGeometry::Radial {
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
            exposure_ev: 0.4,
            ..LocalAdjustment::default()
        }],
        ..EditRecipe::default()
    };
    let replaced = autoshade::xmp::merge_recipe_into_xmp(existing, &replacement).unwrap();

    assert!(replaced.doc.contains("Mask/CircularGradient"));
    assert!(!replaced.doc.contains("Mask/Brush"));
    assert_eq!(replaced.notes.len(), 1);
    assert!(replaced.notes[0].contains("edited mask"));
}

#[test]
fn foreign_crs_alias_is_refused_and_named_in_public_diagnostics() {
    let alias = r#"<rdf:Description
 xmlns:cr="http://ns.adobe.com/camera-raw-settings/1.0/"
 cr:Exposure2012="+1.00"/>"#;
    let rebound = r#"<rdf:Description
 xmlns:crs="http://example.invalid/ns"
 crs:Exposure2012="+1.00"/>"#;

    for (document, diagnostic_fragment) in [(alias, "`cr:`"), (rebound, "xmlns:crs")] {
        assert!(
            autoshade::xmp::merge_recipe_into_xmp(document, &EditRecipe::default()).is_none()
        );
        let diagnostics = autoshade::xmp::unparsable_crs_numbers(document);
        assert_eq!(diagnostics.len(), 1);
        assert!(
            diagnostics[0].contains(diagnostic_fragment),
            "{diagnostics:?}"
        );
    }
}

#[test]
fn ancestor_crs_namespace_and_ratings_only_documents_merge_in_place() {
    let element_form = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/">
<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
 xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/">
 <rdf:Description><crs:Exposure2012>+0.80</crs:Exposure2012></rdf:Description>
</rdf:RDF></x:xmpmeta>"#;
    let ratings = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/">
<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
 <rdf:Description xmlns:xmp="http://ns.adobe.com/xap/1.0/" xmp:Rating="4"/>
</rdf:RDF></x:xmpmeta>"#;
    let recipe = EditRecipe {
        exposure_ev: 0.25,
        ..EditRecipe::default()
    };

    assert_eq!(
        autoshade::xmp::xmp_to_recipe(element_form).exposure_ev,
        0.8
    );
    let element_merged =
        autoshade::xmp::merge_recipe_into_xmp(element_form, &recipe).unwrap();
    assert_eq!(element_merged.doc.matches("<rdf:Description").count(), 1);
    assert!(!element_merged.doc.contains("+0.80"));
    assert_eq!(
        autoshade::xmp::xmp_to_recipe(&element_merged.doc).exposure_ev,
        0.25
    );

    let rating_merged = autoshade::xmp::merge_recipe_into_xmp(ratings, &recipe).unwrap();
    assert!(rating_merged.doc.contains(r#"xmp:Rating="4""#));
    assert_eq!(
        autoshade::xmp::xmp_to_recipe(&rating_merged.doc).exposure_ev,
        0.25
    );
    assert!(rating_merged.notes.is_empty());
}

#[test]
fn xmp_input_size_and_numeric_groups_stop_at_public_recipe_boundaries() {
    let document = r#"<rdf:Description
 xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"
 crs:WhiteBalance="Custom" crs:Temperature="90000"
 crs:Exposure2012="99" crs:Contrast2012="-500" crs:Sharpness="200"
 crs:HasCrop="True" crs:CropLeft="-1" crs:CropTop="0"
 crs:CropRight="1" crs:CropBottom="1">
 <crs:ToneCurvePV2012><rdf:Seq><rdf:li>999, -5</rdf:li></rdf:Seq></crs:ToneCurvePV2012>
 <crs:MaskGroupBasedCorrections><rdf:Seq>
  <rdf:li><rdf:Description crs:What="Correction" crs:LocalExposure2012="9">
   <crs:CorrectionMasks><rdf:Seq>
    <rdf:li crs:What="Mask/Gradient" crs:ZeroX="0.5" crs:ZeroY="0.8"
     crs:FullX="0.5" crs:FullY="0.2"/>
   </rdf:Seq></crs:CorrectionMasks>
  </rdf:Description></rdf:li>
 </rdf:Seq></crs:MaskGroupBasedCorrections>
</rdf:Description>"#;

    let bounded = autoshade::xmp::xmp_to_recipe(document);
    assert_eq!(bounded.temperature_k, Some(50000.0));
    assert_eq!(bounded.exposure_ev, 5.0);
    assert_eq!(bounded.contrast, -100.0);
    assert_eq!(bounded.sharpening, 150.0);
    assert!(bounded.crop.is_none());
    assert!(bounded.tone_curve.is_empty());
    assert!(bounded.masks.is_empty());
    assert_eq!(autoshade::xmp::unsupported_corrections(document), 1);

    let oversized = "x".repeat(16 * 1024 * 1024 + 1);
    assert_eq!(
        autoshade::xmp::xmp_to_recipe(&oversized),
        EditRecipe::default()
    );
    assert!(
        autoshade::xmp::merge_recipe_into_xmp(&oversized, &EditRecipe::default()).is_none()
    );
    assert_eq!(
        autoshade::xmp::unparsable_crs_numbers(&oversized),
        vec!["XMP document exceeds the 16 MiB limit".to_string()]
    );
}

fn named_profile_recipe(scale: f32) -> EditRecipe {
    EditRecipe {
        lens_profile: LensProfile {
            vignette: vec![1.0, 1.5],
            vignette_on: true,
            adobe: Some(AdobeLensProfile {
                name: "Adobe synthetic profile".to_string(),
                filename: "synthetic-profile.lcp".to_string(),
            }),
            ..LensProfile::default()
        },
        lens_profile_vignetting_scale: scale,
        ..EditRecipe::default()
    }
}

#[test]
fn lens_profile_strength_is_named_only_when_lightroom_can_honor_it() {
    let adjusted = named_profile_recipe(50.0);
    let adjusted_xmp = autoshade::xmp::recipe_to_xmp(&adjusted);

    assert_eq!(
        autoshade::xmp::lens_profile_setup(&adjusted_xmp),
        Some(LensProfileSetup::LensDefaults)
    );
    assert_eq!(
        autoshade::xmp::lens_profile_name(&adjusted_xmp).as_deref(),
        Some("Adobe synthetic profile")
    );
    assert_eq!(
        autoshade::xmp::lens_profile_filename(&adjusted_xmp).as_deref(),
        Some("synthetic-profile.lcp")
    );
    assert_eq!(
        autoshade::xmp::xmp_to_recipe(&adjusted_xmp).lens_profile_vignetting_scale,
        50.0
    );

    let default_xmp = autoshade::xmp::recipe_to_xmp(&named_profile_recipe(100.0));
    assert_eq!(autoshade::xmp::lens_profile_setup(&default_xmp), None);
    assert_eq!(autoshade::xmp::lens_profile_name(&default_xmp), None);

    let mut unnamed = named_profile_recipe(50.0);
    unnamed.lens_profile.adobe = None;
    let unnamed_xmp = autoshade::xmp::recipe_to_xmp(&unnamed);
    assert_eq!(
        (
            autoshade::xmp::lens_profile_setup(&unnamed_xmp),
            autoshade::xmp::lens_profile_name(&unnamed_xmp),
        ),
        (None, None)
    );
    assert!(
        autoshade::xmp::global_export_losses(&unnamed)
            .contains(&"lens_profile_vignetting_scale")
    );
}

#[test]
fn foreign_lens_strength_requires_both_profile_setup_and_name() {
    let document = |extra: &str| {
        format!(
            r#"<rdf:Description xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"
 crs:LensProfileEnable="1" {extra}
 crs:LensProfileVignettingScale="50" crs:LensProfileDistortionScale="50"/>"#
        )
    };
    let name = r#"crs:LensProfileName="Adobe synthetic profile""#;

    for (extra, expected) in [
        (String::new(), 100.0),
        (r#"crs:LensProfileSetup="LensDefaults""#.to_string(), 100.0),
        (name.to_string(), 100.0),
        (
            format!(r#"crs:LensProfileSetup="LensDefaults" {name}"#),
            50.0,
        ),
        (
            format!(r#"crs:LensProfileSetup="Custom" {name}"#),
            50.0,
        ),
    ] {
        let imported = autoshade::xmp::xmp_to_recipe(&document(&extra));
        assert_eq!(
            (
                imported.lens_profile_vignetting_scale,
                imported.lens_profile_distortion_scale,
            ),
            (expected, expected),
            "{extra}"
        );
    }
}

#[test]
fn readable_mask_corrections_import_with_named_losses_and_survive_preserving_merge() {
    let document = r#"<rdf:Description
 xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/">
 <crs:MaskGroupBasedCorrections><rdf:Seq>
  <rdf:li><rdf:Description crs:What="Correction">
   <crs:CorrectionMasks><rdf:Seq>
    <rdf:li crs:What="Mask/CircularGradient" crs:Top="0.2" crs:Left="0.2"
     crs:Bottom="0.8" crs:Right="0.8" crs:Feather="50" crs:Roundness="0"
     crs:Flipped="false" crs:Angle="12"/>
   </rdf:Seq></crs:CorrectionMasks>
  </rdf:Description></rdf:li>
  <rdf:li><rdf:Description crs:What="Correction">
   <crs:CorrectionMasks><rdf:Seq><rdf:li crs:What="Mask/Brush"/></rdf:Seq></crs:CorrectionMasks>
  </rdf:Description></rdf:li>
 </rdf:Seq></crs:MaskGroupBasedCorrections>
</rdf:Description>"#;

    let imported = autoshade::xmp::xmp_to_recipe(document);
    let losses = autoshade::xmp::import_losses(document);
    let merged = autoshade::xmp::merge_recipe_into_xmp(
        document,
        &EditRecipe {
            exposure_ev: 0.25,
            ..EditRecipe::default()
        },
    )
    .unwrap();

    assert_eq!(imported.masks.len(), 1);
    assert_eq!(autoshade::xmp::unsupported_corrections(document), 1);
    assert!(losses.iter().any(|loss| {
        loss.reason == MaskImportReason::Rotation(12)
    }));
    assert!(losses.iter().any(|loss| {
        matches!(loss.reason, MaskImportReason::Unrepresentable)
    }));
    assert!(merged.doc.contains("Mask/Brush"));
    assert!(merged.doc.contains(r#"crs:Angle="12""#));
}

#[test]
fn external_ai_component_imports_beside_its_parametric_base() {
    let original = EditRecipe {
        masks: vec![LocalAdjustment {
            mask: MaskGeometry::Linear {
                zero_x: 0.5,
                zero_y: 0.8,
                full_x: 0.5,
                full_y: 0.2,
            },
            components: vec![MaskComponent {
                geometry: MaskGeometry::AiMask {
                    name: "Sky 1".to_string(),
                    subtype: 2,
                    ref_x: 0.605469,
                    ref_y: 0.281525,
                    blend_mode: 0,
                    value: 1.0,
                    inverted: false,
                    mask_version: 1,
                    provenance: vec![
                        ("InputDigest".to_string(), "synthetic-input".to_string()),
                        ("ModelVersion".to_string(), "234881976".to_string()),
                    ],
                    gesture: Vec::new(),
                    raster: None,
                },
                mode: MaskCombine::Add,
                inverted: false,
            }],
            ..LocalAdjustment::default()
        }],
        ..EditRecipe::default()
    };
    let projected = autoshade::xmp::recipe_to_xmp(&original);
    let external = without_autoshade_mask_intent(projected.replacen(
        "asr:Payload=\"1\"",
        "asr:Payload=\"future\"",
        1,
    ));

    let restored = autoshade::xmp::xmp_to_recipe(&external);
    let losses = autoshade::xmp::import_losses(&external);

    assert_eq!(restored.masks.len(), 1);
    assert!(matches!(
        restored.masks[0].mask,
        MaskGeometry::Linear { .. }
    ));
    assert_eq!(restored.masks[0].components.len(), 1);
    assert_eq!(restored.masks[0].components[0].mode, MaskCombine::Add);
    let MaskGeometry::AiMask {
        subtype,
        ref_x,
        ref_y,
        provenance,
        raster,
        ..
    } = &restored.masks[0].components[0].geometry
    else {
        panic!("the AI component remains an AI component");
    };
    assert_eq!((*subtype, *ref_x, *ref_y), (2, 0.605469, 0.281525));
    assert!(provenance.contains(&("InputDigest".to_string(), "synthetic-input".to_string())));
    assert!(raster.is_none());
    assert!(losses
        .iter()
        .any(|loss| loss.reason == MaskImportReason::AiMaskRecomputed));
}

#[test]
fn external_ai_mask_rewrites_its_intent_with_fresh_identity_and_disclosure() {
    let external = r#"<rdf:Description
 xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/">
 <crs:MaskGroupBasedCorrections><rdf:Seq>
  <rdf:li><rdf:Description crs:What="Correction" crs:CorrectionName="sky">
   <crs:CorrectionMasks><rdf:Seq>
    <rdf:li><rdf:Description crs:What="Mask/Image" crs:MaskActive="true"
     crs:MaskName="Sky 1" crs:MaskBlendMode="0" crs:MaskInverted="false"
     crs:MaskSyncID="legacy-sync-id" crs:MaskValue="1" crs:MaskVersion="1"
     crs:MaskSubType="2" crs:ReferencePoint="0.605469 0.281525"
     crs:InputDigest="synthetic-input" crs:MaskDigest="synthetic-mask"
     crs:WholeImageArea="0/1,0/1,1920/1,2880/1"
     crs:ModelVersion="234881976"/>
    </rdf:li>
   </rdf:Seq></crs:CorrectionMasks>
  </rdf:Description></rdf:li>
 </rdf:Seq></crs:MaskGroupBasedCorrections>
</rdf:Description>"#;

    let imported = autoshade::xmp::xmp_to_recipe(external);
    let rewritten = autoshade::xmp::recipe_to_xmp(&imported);
    let losses = autoshade::xmp::mask_export_losses(&imported);

    for expected in [
        r#"crs:What="Mask/Image""#,
        r#"crs:MaskSubType="2""#,
        r#"crs:ReferencePoint="0.605469 0.281525""#,
        r#"crs:MaskVersion="1""#,
        r#"crs:InputDigest="synthetic-input""#,
        r#"crs:MaskDigest="synthetic-mask""#,
        r#"crs:ModelVersion="234881976""#,
    ] {
        assert!(rewritten.contains(expected), "{expected}");
    }
    assert!(!rewritten.contains("legacy-sync-id"));
    assert!(losses
        .iter()
        .any(|loss| loss.reason == MaskLossReason::AiMaskRecomputed));
}

fn external_ai_correction(subtype: u32, extra: &str, quote: char) -> String {
    let attr = |name: &str, value: &str| format!("crs:{name}={quote}{value}{quote}");
    let subtype_and_reference = format!(
        "{} {}",
        attr("MaskSubType", &subtype.to_string()),
        attr("ReferencePoint", "0.605469 0.281525")
    );
    format!(
        r#"<rdf:Description xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/">
<crs:MaskGroupBasedCorrections><rdf:Seq>
 <rdf:li><rdf:Description crs:What="Correction" crs:CorrectionName="AI">
  <crs:CorrectionMasks><rdf:Seq>
   <rdf:li><rdf:Description {} {} {} {} {} {} {} {} {extra}/></rdf:li>
  </rdf:Seq></crs:CorrectionMasks>
 </rdf:Description></rdf:li>
</rdf:Seq></crs:MaskGroupBasedCorrections>
</rdf:Description>"#,
        attr("What", "Mask/Image"),
        attr("MaskActive", "true"),
        attr("MaskName", "Sky 1"),
        attr("MaskBlendMode", "0"),
        attr("MaskInverted", "false"),
        attr("MaskValue", "1"),
        attr("MaskVersion", "1"),
        subtype_and_reference,
    )
}

#[test]
fn ai_masks_outside_the_measured_attribute_vocabulary_are_refused_and_named() {
    let valid = external_ai_correction(2, "", '"');
    assert_eq!(autoshade::xmp::xmp_to_recipe(&valid).masks.len(), 1);

    for invalid in [
        external_ai_correction(2, r#"crs:SomethingNobodyMeasured="1""#, '"'),
        external_ai_correction(7, "", '"'),
    ] {
        assert!(autoshade::xmp::xmp_to_recipe(&invalid).masks.is_empty());
        assert!(autoshade::xmp::import_losses(&invalid)
            .iter()
            .any(|loss| loss.reason.is_drop()));
    }
}

fn correction_scope_fixture(component_extra: &str, stray: &str) -> String {
    format!(
        r#"<x:xmpmeta xmlns:x="adobe:ns:meta/" x:xmptk="AutoShade 2">
<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
<rdf:Description xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/">
 <crs:MaskGroupBasedCorrections><rdf:Seq>
  <rdf:li><rdf:Description crs:What="Correction" crs:CorrectionAmount="1"
   crs:CorrectionActive="true" crs:CorrectionName="Mask 1"
   crs:LocalContrast2012="0.2">
   {stray}
   <crs:CorrectionMasks><rdf:Seq>
    <rdf:li crs:What="Mask/Gradient" crs:MaskActive="true"
     crs:MaskBlendMode="0" crs:MaskInverted="false" crs:MaskValue="1"
     crs:ZeroX="0.5" crs:ZeroY="0.8" crs:FullX="0.5" crs:FullY="0.2"
     {component_extra}/>
   </rdf:Seq></crs:CorrectionMasks>
  </rdf:Description></rdf:li>
 </rdf:Seq></crs:MaskGroupBasedCorrections>
</rdf:Description></rdf:RDF></x:xmpmeta>"#
    )
}

#[test]
fn nested_component_slider_names_cannot_answer_for_the_correction() {
    let document =
        correction_scope_fixture(r#"crs:LocalExposure2012="9""#, "");

    let restored = autoshade::xmp::xmp_to_recipe(&document);

    assert_eq!(restored.masks.len(), 1);
    assert_eq!(restored.masks[0].exposure_ev, 0.0);
    assert_eq!(restored.masks[0].contrast, 20.0);
    assert_eq!(autoshade::xmp::unsupported_corrections(&document), 0);
}

#[test]
fn a_range_mask_belongs_only_to_the_correction_component_list() {
    let range = r#"<rdf:li crs:What="Mask/RangeMask" crs:MaskActive="true"
 crs:MaskBlendMode="1" crs:MaskValue="0" crs:MaskInverted="true"
 crs:LumRange="0 0.2 0.8 1"/>"#;
    let outside = correction_scope_fixture("", range);
    let inside = correction_scope_fixture("", "").replace(
        "</rdf:Seq></crs:CorrectionMasks>",
        &format!("{range}</rdf:Seq></crs:CorrectionMasks>"),
    );

    let outside_recipe = autoshade::xmp::xmp_to_recipe(&outside);
    let inside_recipe = autoshade::xmp::xmp_to_recipe(&inside);

    assert!(outside_recipe.masks[0].range.is_none());
    assert!(matches!(
        inside_recipe.masks[0].range,
        Some(RangeMask::Luminance { .. })
    ));
}

#[test]
fn single_quoted_ai_attributes_carry_provenance_and_still_refuse_unknown_names() {
    let valid = external_ai_correction(
        2,
        "crs:InputDigest='synthetic-input' crs:ModelVersion='234881976'",
        '\'',
    );
    let restored = autoshade::xmp::xmp_to_recipe(&valid);
    let MaskGeometry::AiMask { provenance, .. } = &restored.masks[0].mask else {
        panic!("single-quoted AI geometry imports");
    };

    assert!(provenance.contains(&("InputDigest".to_string(), "synthetic-input".to_string())));
    assert!(provenance.contains(&("ModelVersion".to_string(), "234881976".to_string())));
    let rewritten = autoshade::xmp::recipe_to_xmp(&restored);
    assert!(rewritten.contains(r#"crs:InputDigest="synthetic-input""#));
    assert!(rewritten.contains(r#"crs:ModelVersion="234881976""#));

    let invalid = external_ai_correction(2, "crs:Bogus='1'", '\'');
    assert!(autoshade::xmp::xmp_to_recipe(&invalid).masks.is_empty());
}

fn rotated_radial_document() -> String {
    let recipe = EditRecipe {
        masks: vec![LocalAdjustment {
            mask: MaskGeometry::Radial {
                top: 0.2,
                left: 0.25,
                bottom: 0.8,
                right: 0.75,
                feather: 0.5,
                roundness: 0.0,
                flipped: false,
                angle: 24.0,
                midpoint: 50.0,
                mask_version: 2,
            },
            ..LocalAdjustment::default()
        }],
        ..EditRecipe::default()
    };
    without_recipe_payload(
        autoshade::xmp::recipe_to_xmp_in_frame(
            &recipe,
            autoshade::xmp::FrameAspect::from_size(9504.0, 6336.0),
        )
        .0,
    )
}

#[test]
fn rotated_mask_frame_fields_come_from_one_complete_description_in_both_spellings() {
    let document = rotated_radial_document();
    let expected = autoshade::xmp::xmp_to_recipe(&document).masks[0]
        .mask
        .clone();
    let decoy = document.replacen(
        "<rdf:Description",
        r#"<rdf:Description xmlns:tiff="http://ns.adobe.com/tiff/1.0/" tiff:ImageWidth="6000"/>
<rdf:Description"#,
        1,
    );
    let mixed = document
        .replacen("\n    tiff:ImageLength=\"6336\"", "", 1)
        .replacen(
            "</rdf:Description>",
            "<tiff:ImageLength>6336</tiff:ImageLength></rdf:Description>",
            1,
        );

    assert_eq!(
        autoshade::xmp::xmp_to_recipe(&decoy).masks[0].mask,
        expected
    );
    assert_eq!(
        autoshade::xmp::xmp_to_recipe(&mixed).masks[0].mask,
        expected
    );
}

#[test]
fn rotated_mask_frame_falls_back_across_incomplete_descriptions() {
    let document = rotated_radial_document();
    let expected = autoshade::xmp::xmp_to_recipe(&document).masks[0]
        .mask
        .clone();
    let split = document
        .replacen("\n    tiff:ImageWidth=\"9504\"", "", 1)
        .replacen("\n    tiff:ImageLength=\"6336\"", "", 1)
        .replacen("\n    tiff:Orientation=\"1\"", "", 1)
        .replacen(
            "<rdf:Description",
            r#"<rdf:Description xmlns:tiff="http://ns.adobe.com/tiff/1.0/" tiff:Orientation="1"/>
<rdf:Description tiff:ImageWidth="9504"/>
<rdf:Description tiff:ImageLength="6336"/>
<rdf:Description"#,
            1,
        );

    assert_eq!(
        autoshade::xmp::xmp_to_recipe(&split).masks[0].mask,
        expected
    );
}

fn brush_table_refusal(
    tag: &str,
    companion: Option<&[u8]>,
    token: &str,
    advertised: usize,
) -> (EditRecipe, Vec<String>) {
    let tree = support::TempTree::new(tag);
    let photo = tree.path().join("photo.dng");
    std::fs::write(&photo, b"synthetic photo").unwrap();
    if let Some(bytes) = companion {
        std::fs::write(photo.with_extension("acr"), bytes).unwrap();
    }
    let xmp = format!(
        r#"<rdf:Description xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/">
<crs:MaskGroupBasedCorrections><rdf:Seq>
 <rdf:li><rdf:Description crs:What="Correction" crs:CorrectionName="table brush">
  <crs:CorrectionMasks><rdf:Seq>
   <rdf:li><rdf:Description crs:What="Mask/Aggregate" crs:MaskActive="true"
    crs:MaskName="brush group" crs:MaskBlendMode="0" crs:MaskValue="1"
    crs:MaskBrushTable="{token}" crs:MaskBrushUncompressedBytes="{advertised}"/>
   </rdf:li>
  </rdf:Seq></crs:CorrectionMasks>
 </rdf:Description></rdf:li>
</rdf:Seq></crs:MaskGroupBasedCorrections>
</rdf:Description>"#
    );
    let collector = autoshade::diag::Collector::new();
    let diag = autoshade::diag::Diag::about(&collector, &photo);
    let recipe = autoshade::xmp::xmp_to_recipe_with_diag(&xmp, &diag);
    let lines = collector
        .take()
        .into_iter()
        .map(|line| line.text)
        .collect();
    (recipe, lines)
}

fn one_object_acr(blob: &[u8]) -> (Vec<u8>, String) {
    let key = md5::compute(blob).0;
    let mut acr = Vec::new();
    acr.extend_from_slice(b"ACR\0");
    acr.extend_from_slice(&1u32.to_le_bytes());
    acr.extend_from_slice(b"ARW\0");
    acr.extend_from_slice(&1u32.to_le_bytes());
    acr.extend_from_slice(&0u32.to_le_bytes());
    acr.extend_from_slice(&key);
    acr.extend_from_slice(&(blob.len() as u64).to_le_bytes());
    acr.extend_from_slice(&52u64.to_le_bytes());
    acr.extend_from_slice(blob);
    while acr.len() % 4 != 0 {
        acr.push(0);
    }
    let token = key.iter().map(|byte| format!("{byte:02x}")).collect();
    (acr, token)
}

fn brush_object(stream: &[u8]) -> Vec<u8> {
    let mut object = Vec::with_capacity(16 + stream.len());
    for word in [4u32, 1, 64_000, stream.len() as u32] {
        object.extend_from_slice(&word.to_le_bytes());
    }
    object.extend_from_slice(stream);
    object
}

fn valid_brush_object() -> Vec<u8> {
    use base64::Engine as _;
    let compressed = base64::engine::general_purpose::STANDARD
        .decode("H2QAAGTw5t4FGQ3w9izCvnCADyiQiAPMKQesrbW2JFDJz3DwGLZDucVzKPwAkJ1BPK8hplyqumm7cZqXdetOUBlwfBgMrwL+4eBHgNqstxj8NwFsd2wG")
        .unwrap();
    brush_object(&compressed)
}

const MASK_BRUSH_UNKNOWN_OPCODE: &[u8] = &[
    0x1B, 0x52, 0x00, 0xF8, 0x07, 0x61, 0x73, 0x13, 0xE9, 0x1A, 0xA2, 0xCD, 0x52,
    0xE5, 0xBC, 0x45, 0xD0, 0x05, 0x99, 0x7A, 0xA4, 0x03, 0x01, 0x80, 0x40, 0xA0,
    0x3C, 0x0C, 0x3E, 0xDD, 0x2B, 0xBD, 0x64, 0x08, 0xE4,
];

const MASK_BRUSH_TRAILING_PAYLOAD: &[u8] = &[
    0x1B, 0x57, 0x00, 0xF8, 0x9F, 0x07, 0x76, 0x0C, 0x99, 0x22, 0x68, 0xF8, 0x02,
    0xE9, 0xA5, 0x10, 0x26, 0xF7, 0x24, 0xE1, 0x08, 0xDB, 0x12, 0x4C, 0x23, 0xA8,
    0x84, 0xA0, 0x93, 0xE0, 0x81, 0xBA, 0x12, 0x66, 0x61, 0x03, 0x4E, 0x38, 0x0D,
    0x18, 0x37, 0x5A, 0x66, 0xBF, 0x1C, 0x20, 0xC1, 0x40, 0x03, 0xB5, 0x8C, 0xFB,
    0xE9, 0xD1, 0x02, 0x81, 0xBC, 0x35, 0x01,
];

#[test]
fn missing_mask_brush_companion_is_disclosed_as_unavailable() {
    let (recipe, lines) = brush_table_refusal(
        "brush-table-missing",
        None,
        "00000000000000000000000000000000",
        1,
    );

    assert!(recipe.masks.is_empty());
    assert!(lines
        .iter()
        .any(|line| line.contains("MaskBrushTableUnavailable")));
}

#[test]
fn malformed_mask_brush_companion_is_disclosed_as_container_invalid() {
    let (recipe, lines) = brush_table_refusal(
        "brush-table-malformed",
        Some(b"not an ACR container"),
        "00000000000000000000000000000000",
        1,
    );

    assert!(recipe.masks.is_empty());
    assert!(lines.iter().any(|line| line.contains("ContainerInvalid")));
}

#[test]
fn absent_mask_brush_key_is_disclosed_as_reference_mismatch() {
    let (acr, present) = one_object_acr(b"synthetic brush object");
    let missing = if present == "00000000000000000000000000000000" {
        "11111111111111111111111111111111"
    } else {
        "00000000000000000000000000000000"
    };
    let (recipe, lines) =
        brush_table_refusal("brush-table-reference", Some(&acr), missing, 1);

    assert!(recipe.masks.is_empty());
    assert!(lines.iter().any(|line| line.contains("ReferenceMismatch")));
}

#[test]
fn changed_mask_brush_blob_is_disclosed_as_digest_mismatch() {
    let object = valid_brush_object();
    let (mut acr, token) = one_object_acr(&object);
    acr[52 + object.len() - 1] ^= 1;

    let (recipe, lines) =
        brush_table_refusal("brush-table-digest", Some(&acr), &token, 101);

    assert!(recipe.masks.is_empty());
    assert!(lines.iter().any(|line| line.contains("DigestMismatch")));
}

#[test]
fn unknown_mask_brush_envelope_is_disclosed_as_encoding_unsupported() {
    let mut object = valid_brush_object();
    object[0..4].copy_from_slice(&5u32.to_le_bytes());
    let (acr, token) = one_object_acr(&object);

    let (recipe, lines) =
        brush_table_refusal("brush-table-envelope", Some(&acr), &token, 101);

    assert!(recipe.masks.is_empty());
    assert!(lines
        .iter()
        .any(|line| line.contains("EncodingUnsupported")));
}

#[test]
fn invalid_mask_brush_brotli_is_disclosed_as_corrupt() {
    let (acr, token) = one_object_acr(&brush_object(&[0xFF]));

    let (recipe, lines) =
        brush_table_refusal("brush-table-brotli", Some(&acr), &token, 1);

    assert!(recipe.masks.is_empty());
    assert!(lines.iter().any(|line| line.contains("Corrupt")));
}

#[test]
fn wrong_mask_brush_output_size_is_disclosed_as_length_mismatch() {
    let (acr, token) = one_object_acr(&valid_brush_object());

    let (recipe, lines) =
        brush_table_refusal("brush-table-length", Some(&acr), &token, 102);

    assert!(recipe.masks.is_empty());
    assert!(lines.iter().any(|line| line.contains("LengthMismatch")));
}

#[test]
fn unknown_mask_brush_opcode_is_disclosed_as_payload_unsupported() {
    let (acr, token) =
        one_object_acr(&brush_object(MASK_BRUSH_UNKNOWN_OPCODE));

    let (recipe, lines) =
        brush_table_refusal("brush-table-opcode", Some(&acr), &token, 83);

    assert!(recipe.masks.is_empty());
    assert!(lines
        .iter()
        .any(|line| line.contains("PayloadUnsupported")));
}

#[test]
fn trailing_mask_brush_bytes_are_disclosed_as_payload_invalid() {
    let (acr, token) =
        one_object_acr(&brush_object(MASK_BRUSH_TRAILING_PAYLOAD));

    let (recipe, lines) =
        brush_table_refusal("brush-table-trailing", Some(&acr), &token, 88);

    assert!(recipe.masks.is_empty());
    assert!(lines.iter().any(|line| line.contains("PayloadInvalid")));
}
