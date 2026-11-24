use autoshade::recipe::{
    AdobeLensProfile, BrushStroke, ColorGrade, Crop, CurvePoint, EditRecipe, Hsl, LensProfile,
    LocalAdjustment, MaskCombine, MaskComponent, MaskGeometry, MaskWarpSource, RangeMask,
};
use autoshade::xmp::{LensProfileSetup, MaskImportLoss, MaskImportReason, MaskLossReason};

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

fn lens_profile_switch_document(attribute: &str) -> String {
    format!(
        r#"<rdf:Description xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/" {attribute}/>"#
    )
}

#[test]
fn numeric_and_lowercase_lens_profile_switch_spellings_are_read() {
    for (value, expected) in [("0", false), ("1", true), ("true", true)] {
        let document =
            lens_profile_switch_document(&format!(r#"crs:LensProfileEnable="{value}""#));

        assert_eq!(
            autoshade::xmp::lens_profile_enabled(&document),
            Some(expected)
        );
    }
}

#[test]
fn missing_or_similarly_named_lens_profile_key_has_no_switch_opinion() {
    assert_eq!(
        autoshade::xmp::lens_profile_enabled(&lens_profile_switch_document("")),
        None
    );
    assert_eq!(
        autoshade::xmp::lens_profile_enabled(&lens_profile_switch_document(
            r#"crs:LensProfileName="1""#
        )),
        None
    );
}

#[test]
fn render_time_mask_warp_never_changes_the_xmp_boundary() {
    let brush = MaskGeometry::Brush {
        name: "Brush 1".to_string(),
        blend_mode: 0,
        value: 1.0,
        inverted: false,
        strokes: vec![BrushStroke {
            dabs: "d 0.5 0.5".to_string(),
            radius: 0.3,
            flow: 0.8,
            center_weight: 0.1,
            ..BrushStroke::default()
        }],
    };
    let plain = EditRecipe {
        masks: vec![
            LocalAdjustment {
                mask: MaskGeometry::Linear {
                    zero_x: 0.1,
                    zero_y: 0.2,
                    full_x: 0.9,
                    full_y: 0.8,
                },
                components: vec![
                    MaskComponent {
                        geometry: brush,
                        mode: MaskCombine::Add,
                        inverted: false,
                    },
                    MaskComponent {
                        geometry: MaskGeometry::Radial {
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
                        mode: MaskCombine::Add,
                        inverted: false,
                    },
                ],
                ..LocalAdjustment::default()
            },
        ],
        ..EditRecipe::default()
    };
    let mut warped = plain.clone();
    warped.lens_profile = LensProfile {
        mask_warp: vec![1.05, 1.03, 1.01, 0.99, 0.97],
        mask_warp_src: MaskWarpSource::Lcp,
        ..LensProfile::default()
    };
    assert!(warped.lens_profile.mask_warp[0] > 1.04);
    assert!(warped.lens_profile.mask_warp.last().copied().unwrap() < 0.98);

    let plain_xmp = autoshade::xmp::recipe_to_xmp(&plain);
    let warped_xmp = autoshade::xmp::recipe_to_xmp(&warped);
    assert_eq!(
        xmp_element_block(&warped_xmp, "crs:MaskGroupBasedCorrections"),
        xmp_element_block(&plain_xmp, "crs:MaskGroupBasedCorrections")
    );

    let reopened =
        autoshade::xmp::xmp_to_recipe(&without_recipe_payload(warped_xmp));
    assert_eq!(reopened.masks.len(), plain.masks.len());
    assert_eq!(reopened.masks[0].mask, plain.masks[0].mask);
    assert_eq!(
        reopened.masks[0].components[1].geometry,
        plain.masks[0].components[1].geometry
    );
    let MaskGeometry::Brush { strokes, .. } = &reopened.masks[0].components[0].geometry else {
        panic!("brush component survives")
    };
    assert_eq!(strokes[0].dabs, "d 0.5 0.5");
    assert_eq!(
        (
            strokes[0].radius,
            strokes[0].flow,
            strokes[0].center_weight,
        ),
        (0.3, 0.8, 0.1)
    );
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

fn many_object_acr(objects: &[Vec<u8>]) -> (Vec<u8>, Vec<String>) {
    let directory_end = 20 + 32 * objects.len();
    let mut body = Vec::new();
    let mut entries = Vec::new();
    let mut tokens = Vec::new();
    for object in objects {
        while !(directory_end + body.len()).is_multiple_of(4) {
            body.push(0);
        }
        let key = md5::compute(object).0;
        entries.push((key, object.len() as u64, (directory_end + body.len()) as u64));
        tokens.push(key.iter().map(|byte| format!("{byte:02x}")).collect());
        body.extend_from_slice(object);
    }
    while !(directory_end + body.len()).is_multiple_of(4) {
        body.push(0);
    }
    let mut acr = Vec::new();
    acr.extend_from_slice(b"ACR\0");
    acr.extend_from_slice(&1u32.to_le_bytes());
    acr.extend_from_slice(b"ARW\0");
    acr.extend_from_slice(&(objects.len() as u32).to_le_bytes());
    acr.extend_from_slice(&0u32.to_le_bytes());
    for (key, len, offset) in entries {
        acr.extend_from_slice(&key);
        acr.extend_from_slice(&len.to_le_bytes());
        acr.extend_from_slice(&offset.to_le_bytes());
    }
    acr.extend_from_slice(&body);
    (acr, tokens)
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

const MASK_BRUSH_TABLE_WORD: &[u8] =
    &[0x1B, 0x07, 0x00, 0xF8, 0xA7, 0x00, 0x04, 0x82, 0x92, 0x40, 0x20];
const MASK_BRUSH_RECORD_BOUND: &[u8] =
    &[0x8B, 0x03, 0x80, 0x01, 0x00, 0x00, 0x00, 0x01, 0x01, 0x00, 0x00, 0x03];
const MASK_BRUSH_D_COUNT_BOUND: &[u8] = &[
    0x1B, 0x4D, 0x00, 0xF8, 0x07, 0xE1, 0x64, 0x17, 0x12, 0x21, 0x6A, 0x4A, 0x35,
    0x1B, 0xD8, 0x80, 0x13, 0x4E, 0x03, 0x87, 0x05, 0x06, 0x5A, 0x4E, 0x07, 0x02,
    0x68, 0xC9, 0xC0, 0x02, 0xFD, 0xBC, 0xDA, 0x4B, 0x35, 0x14, 0x78, 0xAF,
];
const MASK_BRUSH_TOKEN_BOUND: &[u8] = &[
    0x5B, 0x5B, 0x00, 0x85, 0x7F, 0x28, 0xF0, 0x76, 0x5F, 0x54, 0x42, 0xD4, 0x94,
    0x6A, 0x82, 0x76, 0x44, 0x97, 0xAE, 0x7E, 0x93, 0x08, 0x5C, 0xC0, 0x55, 0x49,
    0x08, 0x10, 0x80, 0x8D, 0x4F, 0xF7, 0x4D, 0xEB, 0xD0, 0x7D, 0xEF, 0x09, 0x20,
    0x84, 0xB1, 0x1F, 0x08,
];
const MASK_BRUSH_GOOD_A: &[u8] = &[
    0x1B, 0xB8, 0x00, 0xF8, 0x8F, 0xC2, 0xB6, 0xB5, 0x73, 0x94, 0x79, 0x28, 0xD3,
    0x42, 0xF8, 0xC9, 0x20, 0x88, 0x9B, 0xDF, 0xC6, 0x02, 0xEA, 0x3A, 0x0F, 0x6C,
    0x2C, 0x91, 0x28, 0x0E, 0x3C, 0xF0, 0x31, 0x51, 0xD6, 0x46, 0xAC, 0x01, 0x14,
    0x4E, 0xC4, 0xC3, 0x06, 0x9C, 0xA8, 0x07, 0x1E, 0xA0, 0x47, 0x32, 0xDD, 0x01,
    0x20, 0x10, 0xC7, 0x27, 0x96, 0x08, 0x80, 0x08, 0x00, 0x08, 0x00, 0x00, 0x58,
    0x10, 0xF9, 0xEE, 0xDC, 0x49, 0x6B, 0xC2, 0x58, 0x07, 0x20, 0x02, 0x42, 0x78,
    0x81, 0x98, 0x81, 0x5A, 0xD8, 0xAA, 0xBC, 0x89, 0xFA, 0x9B, 0xAA, 0x71, 0x28,
    0x13, 0x13, 0xC2, 0x58, 0xB7, 0xC6, 0x30, 0x36, 0x54, 0xBF, 0x44, 0x93, 0x3B,
    0x1A,
];
const MASK_BRUSH_GOOD_B: &[u8] = &[
    0x1B, 0x56, 0x00, 0xF8, 0x9F, 0x07, 0x76, 0x0C, 0x99, 0x22, 0x68, 0xF8, 0x02,
    0xE9, 0xA5, 0x10, 0x26, 0xF7, 0x24, 0xE1, 0x08, 0xDB, 0x12, 0x4C, 0x23, 0xA8,
    0x84, 0xA0, 0x93, 0xE0, 0x81, 0xBA, 0x12, 0x66, 0x61, 0x03, 0x4E, 0x38, 0x0D,
    0x14, 0x47, 0x5A, 0x66, 0xBF, 0x1C, 0x20, 0xC1, 0x40, 0x03, 0xB5, 0x8C, 0xFB,
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

#[test]
fn non_one_mask_brush_table_word_is_disclosed_as_payload_unsupported() {
    let (acr, token) =
        one_object_acr(&brush_object(MASK_BRUSH_TABLE_WORD));

    let (recipe, lines) =
        brush_table_refusal("brush-table-word", Some(&acr), &token, 8);

    assert!(recipe.masks.is_empty());
    assert!(lines.iter().any(|line| {
        line.contains("PayloadUnsupported") && line.contains("table word is not 1")
    }));
}

#[test]
fn mask_brush_record_count_limit_is_disclosed_before_allocation() {
    let (acr, token) =
        one_object_acr(&brush_object(MASK_BRUSH_RECORD_BOUND));

    let (recipe, lines) =
        brush_table_refusal("brush-table-record-bound", Some(&acr), &token, 8);

    assert!(recipe.masks.is_empty());
    assert!(lines.iter().any(|line| {
        line.contains("PayloadInvalid") && line.contains("record count exceeds")
    }));
}

#[test]
fn mask_brush_d_count_limit_is_disclosed_before_token_walk() {
    let (acr, token) =
        one_object_acr(&brush_object(MASK_BRUSH_D_COUNT_BOUND));

    let (recipe, lines) =
        brush_table_refusal("brush-table-d-count-bound", Some(&acr), &token, 78);

    assert!(recipe.masks.is_empty());
    assert!(lines.iter().any(|line| {
        line.contains("PayloadInvalid") && line.contains("d-count exceeds")
    }));
}

#[test]
fn mask_brush_token_count_limit_covers_non_dab_state_tokens() {
    let (acr, token) =
        one_object_acr(&brush_object(MASK_BRUSH_TOKEN_BOUND));

    let (recipe, lines) =
        brush_table_refusal("brush-table-token-bound", Some(&acr), &token, 327_772);

    assert!(recipe.masks.is_empty());
    assert!(lines.iter().any(|line| {
        line.contains("PayloadInvalid") && line.contains("token count exceeds")
    }));
}

#[test]
fn one_bad_mask_brush_table_does_not_take_down_an_independent_table() {
    let objects = vec![
        valid_brush_object(),
        brush_object(MASK_BRUSH_UNKNOWN_OPCODE),
    ];
    let (acr, tokens) = many_object_acr(&objects);
    let tree = support::TempTree::new("brush-table-independent");
    let photo = tree.path().join("photo.dng");
    std::fs::write(&photo, b"synthetic photo").unwrap();
    std::fs::write(photo.with_extension("acr"), acr).unwrap();
    let inline = r#"<rdf:li><rdf:Description crs:What="Mask/Paint"
 crs:MaskActive="true" crs:MaskBlendMode="0" crs:MaskInverted="false"
 crs:MaskSyncID="1111111111111111111111111111111C" crs:MaskValue="1"
 crs:Radius="0.5" crs:Flow="1" crs:CenterWeight="0">
 <crs:Dabs><rdf:Seq><rdf:li>d 0.25 0.75</rdf:li></rdf:Seq></crs:Dabs>
</rdf:Description></rdf:li>"#;
    let correction = |name: &str, token: &str, expected: usize, body: &str| {
        format!(
            r#"<rdf:li><rdf:Description crs:What="Correction" crs:CorrectionName="{name}">
<crs:CorrectionMasks><rdf:Seq>
 <rdf:li><rdf:Description crs:What="Mask/Aggregate" crs:MaskActive="true"
  crs:MaskName="{name}" crs:MaskBlendMode="0" crs:MaskInverted="false"
  crs:MaskValue="1" crs:MaskBrushTable="{token}"
  crs:MaskBrushUncompressedBytes="{expected}">
  <crs:Masks><rdf:Seq>{body}</rdf:Seq></crs:Masks>
 </rdf:Description></rdf:li>
</rdf:Seq></crs:CorrectionMasks>
</rdf:Description></rdf:li>"#
        )
    };
    let xmp = format!(
        r#"<rdf:Description xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/">
<crs:MaskGroupBasedCorrections><rdf:Seq>{}{}</rdf:Seq></crs:MaskGroupBasedCorrections>
</rdf:Description>"#,
        correction("Good", &tokens[0], 101, ""),
        correction("Bad", &tokens[1], 83, inline),
    );
    let collector = autoshade::diag::Collector::new();
    let diag = autoshade::diag::Diag::about(&collector, &photo);

    let recipe = autoshade::xmp::xmp_to_recipe_with_diag(&xmp, &diag);
    let lines = collector.take();

    assert_eq!(recipe.masks.len(), 2, "{recipe:?}\n{lines:?}");
    let MaskGeometry::Brush {
        strokes: good, ..
    } = &recipe.masks[0].mask
    else {
        panic!("good table imports as a brush");
    };
    let MaskGeometry::Brush {
        strokes: survivor,
        ..
    } = &recipe.masks[1].mask
    else {
        panic!("bad table correction keeps its inline brush");
    };
    assert_eq!(good.len(), 1);
    assert_eq!(survivor.len(), 1);
    assert_eq!(
        survivor[0].sync_id,
        "1111111111111111111111111111111C"
    );
    assert!(lines
        .iter()
        .any(|line| line.text.contains("PayloadUnsupported")));
}

#[test]
fn global_projection_contains_representable_controls_and_names_bitmap_losses() {
    let recipe = EditRecipe {
        exposure_ev: -0.8,
        temperature_k: Some(9000.0),
        tint: 10.0,
        saturation: 30.0,
        tone_curve: vec![
            CurvePoint {
                input: 0,
                output: 0,
            },
            CurvePoint {
                input: 128,
                output: 112,
            },
            CurvePoint {
                input: 255,
                output: 255,
            },
        ],
        masks: vec![LocalAdjustment {
            name: "legacy sky".to_string(),
            mask: MaskGeometry::Bitmap {
                path: "missing-atmosphere-sky.png".to_string(),
            },
            ..LocalAdjustment::default()
        }],
        ..EditRecipe::default()
    };

    let (xmp, losses) = autoshade::xmp::recipe_to_xmp_with_losses(&recipe);
    let native = without_recipe_payload(xmp);
    let projected = autoshade::xmp::xmp_to_recipe(&native);

    for expected in [
        r#"crs:Exposure2012="-0.80""#,
        r#"crs:Temperature="9000""#,
        r#"crs:Tint="+10""#,
        r#"crs:Saturation="+30""#,
        "crs:ToneCurvePV2012",
    ] {
        assert!(native.contains(expected), "{expected}");
    }
    assert!(!native.contains("ToneCurvePV2012Red"));
    assert!(!native.contains("MaskGroupBasedCorrections"));
    assert_eq!(
        losses
            .iter()
            .filter(|loss| loss.reason == MaskLossReason::Bitmap)
            .count(),
        1
    );
    assert_eq!(
        losses
            .iter()
            .filter(|loss| loss.reason == MaskLossReason::RasterNotEmbedded)
            .count(),
        1
    );
    assert_eq!(projected.exposure_ev, -0.8);
    assert_eq!(projected.temperature_k, Some(9000.0));
    assert_eq!(projected.tint, 10.0);
    assert_eq!(projected.saturation, 30.0);
    assert_eq!(projected.tone_curve, recipe.tone_curve);
    assert!(projected.masks.is_empty());
}

#[test]
fn export_losses_name_only_non_neutral_unrepresentable_globals() {
    assert!(autoshade::xmp::global_export_losses(&EditRecipe::default()).is_empty());
    let with_base = EditRecipe {
        base_curve: vec![[0.0, 0.0], [0.5, 0.55], [1.0, 1.0]],
        ..EditRecipe::default()
    };
    assert_eq!(
        autoshade::xmp::global_export_losses(&with_base),
        vec!["base_curve"]
    );
    let with_lens = EditRecipe {
        lens_profile: LensProfile {
            vignette_on: true,
            vignette: vec![1.0, 0.1],
            ..LensProfile::default()
        },
        ..with_base
    };
    assert_eq!(
        autoshade::xmp::global_export_losses(&with_lens),
        vec!["base_curve", "lens_profile"]
    );
    assert!(autoshade::xmp::global_export_losses(&EditRecipe {
        exposure_ev: 1.5,
        ..EditRecipe::default()
    })
    .is_empty());
}

#[test]
fn imported_unmodelled_globals_exclude_owned_keys_and_attribute_values() {
    let document = r#"<rdf:Description
 xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"
 crs:Exposure2012="+1.00" crs:Texture="+30"
 crs:CurveRefineSaturation="100" crs:PerspectiveUpright="1"
 crs:CameraProfile="Adobe Standard"
 crs:CameraProfileDigest="synthetic-digest"
 crs:RawFileName="crs:NotAnAttribute=1.ARW"/>"#;

    let found = autoshade::xmp::unmodelled_global_crs(document);

    assert!(found.contains(&"CurveRefineSaturation".to_string()));
    assert!(found.contains(&"CameraProfileDigest".to_string()));
    for modelled in [
        "Exposure2012",
        "Texture",
        "PerspectiveUpright",
        "CameraProfile",
        "NotAnAttribute",
    ] {
        assert!(!found.contains(&modelled.to_string()), "{modelled}");
    }
    assert!(autoshade::xmp::unmodelled_global_crs("").is_empty());
    assert!(autoshade::xmp::unmodelled_global_crs("<not xml").is_empty());
}

#[test]
fn unmodelled_global_disclosure_reads_attribute_and_property_element_forms() {
    let element = r#"<rdf:Description
 xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/">
 <crs:Exposure2012>+1.00</crs:Exposure2012>
 <crs:Texture>+30</crs:Texture>
 <crs:PointColor>0</crs:PointColor>
 <crs:UprightTransform>1.0000000</crs:UprightTransform>
</rdf:Description>"#;
    let mixed = r#"<rdf:Description
 xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"
 crs:Exposure2012="+1.00" crs:PointColor="0">
 <crs:UprightTransform>1.0000000</crs:UprightTransform>
</rdf:Description>"#;
    let nested = r#"<rdf:Description
 xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/">
 <crs:PointColor>0</crs:PointColor>
 <crs:Look><rdf:Description><crs:Parameters><rdf:Description>
  <crs:LookClarity2012>+50</crs:LookClarity2012>
 </rdf:Description></crs:Parameters></rdf:Description></crs:Look>
 <crs:MaskGroupBasedCorrections><rdf:Seq><rdf:li>
  <rdf:Description crs:LocalExposure2012="0.5" crs:LocalTexture="20"/>
 </rdf:li></rdf:Seq></crs:MaskGroupBasedCorrections>
</rdf:Description>"#;

    assert_eq!(
        autoshade::xmp::unmodelled_global_crs(element),
        vec!["PointColor", "UprightTransform"]
    );
    assert_eq!(
        autoshade::xmp::unmodelled_global_crs(mixed),
        vec!["PointColor", "UprightTransform"]
    );
    assert_eq!(
        autoshade::xmp::unmodelled_global_crs(nested),
        vec!["Look", "PointColor"]
    );
}

#[test]
fn unmodelled_global_names_survive_all_xml_whitespace_before_equals() {
    let document = "<rdf:Description \
        xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\" \
        crs:Exposure2012 = \"+1.00\" \
        crs:CurveRefineSaturation\t=\t\"100\"\n\
        crs:CameraProfileDigest\r\n=\n\"synthetic-digest\"/>";

    assert_eq!(
        autoshade::xmp::unmodelled_global_crs(document),
        vec!["CameraProfileDigest", "CurveRefineSaturation"]
    );
}

#[test]
fn conflicting_crs_binding_refuses_import_merge_and_secondary_loss_channels() {
    let canonical = r#"<rdf:Description
 xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"
 crs:Exposure2012="+2.50" crs:CurveRefineSaturation="100">
 <crs:MaskGroupBasedCorrections><rdf:Seq>
  <rdf:li><rdf:Description crs:What="Correction" crs:CorrectionActive="false">
   <crs:CorrectionMasks><rdf:Seq>
    <rdf:li crs:What="Mask/Gradient" crs:ZeroX="0.5" crs:ZeroY="0.8"
     crs:FullX="0.5" crs:FullY="0.2"/>
   </rdf:Seq></crs:CorrectionMasks>
  </rdf:Description></rdf:li>
 </rdf:Seq></crs:MaskGroupBasedCorrections>
</rdf:Description>"#;
    assert_eq!(autoshade::xmp::unsupported_corrections(canonical), 1);
    assert!(!autoshade::xmp::import_losses(canonical).is_empty());
    assert!(autoshade::xmp::unmodelled_global_crs(canonical)
        .contains(&"CurveRefineSaturation".to_string()));
    let conflict = canonical.replace(
        "http://ns.adobe.com/camera-raw-settings/1.0/",
        "urn:other",
    );

    assert_eq!(
        autoshade::xmp::xmp_to_recipe(&conflict),
        EditRecipe::default()
    );
    assert!(
        autoshade::xmp::merge_recipe_into_xmp(&conflict, &EditRecipe::default()).is_none()
    );
    assert_eq!(autoshade::xmp::unsupported_corrections(&conflict), 0);
    assert!(autoshade::xmp::import_losses(&conflict).is_empty());
    assert!(autoshade::xmp::unmodelled_global_crs(&conflict).is_empty());
    assert!(autoshade::xmp::unparsable_crs_numbers(&conflict)[0]
        .contains("not imported"));
}

#[test]
fn default_namespace_bound_to_camera_raw_is_refused_publicly() {
    let document = r#"<rdf:Description
 xmlns="http://ns.adobe.com/camera-raw-settings/1.0/">
 <Exposure2012>+1.00</Exposure2012>
</rdf:Description>"#;

    assert_eq!(
        autoshade::xmp::xmp_to_recipe(document),
        EditRecipe::default()
    );
    assert!(
        autoshade::xmp::merge_recipe_into_xmp(document, &EditRecipe::default()).is_none()
    );
    assert!(autoshade::xmp::unparsable_crs_numbers(document)[0]
        .contains("DEFAULT namespace"));
}

#[test]
fn nested_crs_rebind_refuses_only_while_a_name_resolves_through_its_scope() {
    let unused = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/">
<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
 <rdf:Description xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"
  crs:Exposure2012="+0.50">
  <dc:island xmlns:dc="http://purl.org/dc/elements/1.1/"
   xmlns:crs="urn:other"><dc:note>hi</dc:note></dc:island>
 </rdf:Description>
</rdf:RDF></x:xmpmeta>"#;
    let used = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/">
<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
 <dc:island xmlns:dc="http://purl.org/dc/elements/1.1/"
  xmlns:crs="urn:other"><dc:inner crs:Shadows2012="+10"/></dc:island>
</rdf:RDF></x:xmpmeta>"#;
    let closed = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/">
<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
 xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/">
 <dc:island xmlns:dc="http://purl.org/dc/elements/1.1/"
  xmlns:crs="urn:other"><dc:note>hi</dc:note></dc:island>
 <rdf:Description crs:Exposure2012="+0.50"/>
</rdf:RDF></x:xmpmeta>"#;

    assert_eq!(autoshade::xmp::xmp_to_recipe(unused).exposure_ev, 0.5);
    assert!(
        autoshade::xmp::merge_recipe_into_xmp(unused, &EditRecipe::default()).is_some()
    );
    assert_eq!(
        autoshade::xmp::xmp_to_recipe(used),
        EditRecipe::default()
    );
    assert!(
        autoshade::xmp::merge_recipe_into_xmp(used, &EditRecipe::default()).is_none()
    );
    assert!(autoshade::xmp::unparsable_crs_numbers(used)[0].contains("urn:other"));
    assert_eq!(autoshade::xmp::xmp_to_recipe(closed).exposure_ev, 0.5);
}

#[test]
fn foreign_camera_raw_alias_refuses_only_when_used() {
    let unused = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/">
<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
 xmlns:zzz="http://ns.adobe.com/camera-raw-settings/1.0/">
 <rdf:Description/>
</rdf:RDF></x:xmpmeta>"#;
    let used = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/">
<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
 xmlns:zzz="http://ns.adobe.com/camera-raw-settings/1.0/">
 <rdf:Description zzz:Exposure2012="+1.00"/>
</rdf:RDF></x:xmpmeta>"#;

    assert!(
        autoshade::xmp::merge_recipe_into_xmp(unused, &EditRecipe::default()).is_some()
    );
    assert!(
        autoshade::xmp::merge_recipe_into_xmp(used, &EditRecipe::default()).is_none()
    );
    assert!(autoshade::xmp::unparsable_crs_numbers(used)[0].contains("`zzz:`"));
}

#[test]
fn merge_skips_an_unused_foreign_crs_description_and_inserts_canonical_settings() {
    let document = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/">
<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
 <rdf:Description xmlns:crs="urn:other"/>
</rdf:RDF></x:xmpmeta>"#;

    let merged = autoshade::xmp::merge_recipe_into_xmp(
        document,
        &EditRecipe {
            exposure_ev: 0.5,
            ..EditRecipe::default()
        },
    )
    .unwrap();

    assert!(merged.doc.contains(r#"xmlns:crs="urn:other""#));
    assert!(merged
        .doc
        .contains(r#"xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/""#));
    assert_eq!(merged.doc.matches("<rdf:Description").count(), 2);
    assert_eq!(autoshade::xmp::xmp_to_recipe(&merged.doc).exposure_ev, 0.5);
}

#[test]
fn namespace_nesting_past_the_tracking_bound_refuses_publicly() {
    let mut document = String::new();
    for _ in 0..1025 {
        document.push_str("<t xmlns:q=\"urn:x\">");
    }

    assert!(
        autoshade::xmp::merge_recipe_into_xmp(&document, &EditRecipe::default()).is_none()
    );
    assert!(autoshade::xmp::unparsable_crs_numbers(&document)[0]
        .contains("more xmlns declarations"));
}

#[test]
fn mismatched_close_cannot_release_a_foreign_binding_early() {
    let document = r#"<rdf:Description
 xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
 xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/">
 <island xmlns:crs="urn:foreign"></bogus>
 <crs:Exposure2012>+2.0</crs:Exposure2012>
 </island>
</rdf:Description>"#;

    assert_eq!(
        autoshade::xmp::xmp_to_recipe(document),
        EditRecipe::default()
    );
    assert!(
        autoshade::xmp::merge_recipe_into_xmp(document, &EditRecipe::default()).is_none()
    );
    assert!(autoshade::xmp::unparsable_crs_numbers(document)[0]
        .contains("urn:foreign"));
}

#[test]
fn flat_namespace_declaration_flood_refuses_publicly() {
    let mut document = String::from("<t");
    for index in 0..257 {
        document.push_str(&format!(r#" xmlns:q{index}="urn:x""#));
    }
    document.push('>');

    assert!(
        autoshade::xmp::merge_recipe_into_xmp(&document, &EditRecipe::default()).is_none()
    );
    assert!(autoshade::xmp::unparsable_crs_numbers(&document)[0]
        .contains("more xmlns declarations"));
}

fn xmp_attribute<'a>(xmp: &'a str, name: &str) -> &'a str {
    let marker = format!("{name}=\"");
    let value = xmp.find(&marker).unwrap() + marker.len();
    let end = value + xmp[value..].find('"').unwrap();
    &xmp[value..end]
}

fn xmp_public_value<'a>(xmp: &'a str, name: &str) -> Option<&'a str> {
    let marker = format!("{name}=\"");
    if let Some(start) = xmp.find(&marker) {
        let value = start + marker.len();
        let end = value + xmp[value..].find('"')?;
        return Some(&xmp[value..end]);
    }
    let open = format!("<{name}>");
    let value = xmp.find(&open)? + open.len();
    let close = format!("</{name}>");
    let end = value + xmp[value..].find(&close)?;
    Some(&xmp[value..end])
}

fn declared_public_frame(xmp: &str) -> Option<autoshade::xmp::FrameAspect> {
    let number = |name: &str| xmp_public_value(xmp, name)?.parse::<f64>().ok();
    let orientation = match number("tiff:Orientation").unwrap_or(1.0) as u8 {
        1 => rawler::Orientation::Normal,
        2 => rawler::Orientation::HorizontalFlip,
        3 => rawler::Orientation::Rotate180,
        4 => rawler::Orientation::VerticalFlip,
        5 => rawler::Orientation::Transpose,
        6 => rawler::Orientation::Rotate90,
        7 => rawler::Orientation::Transverse,
        8 => rawler::Orientation::Rotate270,
        _ => return None,
    };
    autoshade::xmp::FrameAspect::from_size_turned(
        number("tiff:ImageWidth")?,
        number("tiff:ImageLength")?,
        orientation,
    )
}

#[test]
fn whitespace_spelled_curve_items_are_public_curve_points() {
    let document = r#"<rdf:Description
 xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/">
 <crs:ToneCurvePV2012><rdf:Seq>
  <rdf:li >128, 64</rdf:li >
  <rdf:li>255, 255</rdf:li>
 </rdf:Seq></crs:ToneCurvePV2012>
</rdf:Description>"#;

    assert_eq!(
        autoshade::xmp::xmp_to_recipe(document).tone_curve,
        vec![
            CurvePoint {
                input: 128,
                output: 64,
            },
            CurvePoint {
                input: 255,
                output: 255,
            },
        ]
    );
}

#[test]
fn inconsistent_crop_groups_are_disclosed_without_partial_import() {
    let missing = r#"<rdf:Description
 xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"
 crs:HasCrop="True" crs:CropLeft="0.1" crs:CropTop="0.1" crs:CropRight="0.9"/>"#;
    let inverted = r#"<rdf:Description
 xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"
 crs:HasCrop="True" crs:CropLeft="0.8" crs:CropTop="0.1"
 crs:CropRight="0.2" crs:CropBottom="0.9"/>"#;
    let valid = r#"<rdf:Description
 xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"
 crs:HasCrop="True" crs:CropLeft="0.1" crs:CropTop="0.1"
 crs:CropRight="0.9" crs:CropBottom="0.9"/>"#;

    for invalid in [missing, inverted] {
        assert!(autoshade::xmp::xmp_to_recipe(invalid).crop.is_none());
        assert!(autoshade::xmp::unparsable_crs_numbers(invalid)
            .iter()
            .any(|key| key.starts_with("Crop")));
    }
    assert!(autoshade::xmp::xmp_to_recipe(valid).crop.is_some());
    assert!(autoshade::xmp::unparsable_crs_numbers(valid).is_empty());
}

#[test]
fn rotated_left_over_right_crop_is_imported_without_false_disclosure() {
    let recipe = EditRecipe {
        crop: Some(Crop {
            left: 0.30,
            top: 0.10,
            right: 0.62,
            bottom: 0.90,
        }),
        straighten_deg: -35.0,
        ..EditRecipe::default()
    };
    let document = without_recipe_payload(
        autoshade::xmp::recipe_to_xmp_in_frame(
            &recipe,
            autoshade::xmp::FrameAspect::from_size(9504.0, 6336.0),
        )
        .0,
    );
    let left = xmp_attribute(&document, "crs:CropLeft")
        .parse::<f32>()
        .unwrap();
    let right = xmp_attribute(&document, "crs:CropRight")
        .parse::<f32>()
        .unwrap();
    let angle = xmp_attribute(&document, "crs:CropAngle").to_string();

    assert!(left > right);
    let restored = autoshade::xmp::xmp_to_recipe(&document);
    assert!(restored.crop.is_some());
    assert!((restored.straighten_deg + 35.0).abs() < 1e-4);
    assert!(autoshade::xmp::unparsable_crs_numbers(&document).is_empty());

    let flat = document.replacen(
        &format!(r#"crs:CropAngle="{angle}""#),
        r#"crs:CropAngle="0""#,
        1,
    );
    assert!(autoshade::xmp::xmp_to_recipe(&flat).crop.is_none());
    assert!(autoshade::xmp::unparsable_crs_numbers(&flat)
        .iter()
        .any(|key| key.starts_with("Crop")));
}

#[test]
fn attribute_control_characters_use_references_and_round_trip() {
    let value = "a\tb\nc\rd";
    let recipe = EditRecipe {
        masks: vec![LocalAdjustment {
            name: value.to_string(),
            ..LocalAdjustment::default()
        }],
        ..EditRecipe::default()
    };

    let xmp = autoshade::xmp::recipe_to_xmp(&recipe);
    let restored = autoshade::xmp::xmp_to_recipe(&xmp);

    assert!(xmp.contains("a&#9;b&#10;c&#13;d"));
    assert_eq!(restored.masks[0].name, value);
}

#[test]
fn text_constructs_cannot_truncate_the_description_during_merge() {
    for construct in [
        "<!-- </rdf:Description> -->",
        "<![CDATA[ </rdf:Description> ]]>",
        "<?probe </rdf:Description> ?>",
    ] {
        let document = format!(
            r#"<rdf:Description xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/">{construct}<crs:ForeignKeep>25</crs:ForeignKeep></rdf:Description>"#
        );
        let merged = autoshade::xmp::merge_recipe_into_xmp(
            &document,
            &EditRecipe {
                exposure_ev: 0.25,
                ..EditRecipe::default()
            },
        )
        .expect("text containing a close marker remains mergeable");

        assert!(merged.doc.contains(construct));
        assert!(merged.doc.contains("<crs:ForeignKeep>25</crs:ForeignKeep>"));
        assert_eq!(autoshade::xmp::xmp_to_recipe(&merged.doc).exposure_ev, 0.25);
    }
}

#[test]
fn an_unterminated_text_construct_refuses_a_preserving_merge() {
    let document = r#"<rdf:Description xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"><!-- </rdf:Description>"#;

    assert!(
        autoshade::xmp::merge_recipe_into_xmp(document, &EditRecipe::default()).is_none()
    );
}

#[test]
fn construct_heavy_xmp_finishes_preserving_merge_within_the_resource_budget() {
    const BUDGET: std::time::Duration = std::time::Duration::from_secs(5);
    let head =
        r#"<rdf:Description xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/">"#;
    let comments = format!(
        "{head}{}<crs:ForeignKeep>25</crs:ForeignKeep></rdf:Description>",
        "<!--x-->".repeat(80_000)
    );
    let processing_instructions = format!(
        "{head}{}<crs:ForeignKeep>25</crs:ForeignKeep></rdf:Description>",
        "<?p?>".repeat(80_000)
    );

    for (name, document) in [
        ("comments", comments),
        ("processing instructions", processing_instructions),
    ] {
        let started = std::time::Instant::now();
        let merged = autoshade::xmp::merge_recipe_into_xmp(&document, &EditRecipe::default())
            .unwrap_or_else(|| panic!("{name}: valid document was refused"));
        let elapsed = started.elapsed();

        assert!(merged.doc.contains("<crs:ForeignKeep>25</crs:ForeignKeep>"));
        assert!(
            elapsed < BUDGET,
            "{name}: {} bytes took {elapsed:?}, over the {BUDGET:?} budget",
            document.len()
        );
    }
}

fn creative_look_document() -> &'static str {
    r#"<rdf:Description
 xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"
 crs:Exposure2012="+0.20">
 <crs:Look><rdf:Description crs:Name="Adobe Landscape">
  <crs:Parameters><rdf:Description>
   <crs:Clarity2012>+50</crs:Clarity2012>
   <crs:Vibrance>+35</crs:Vibrance>
   <crs:ConvertToGrayscale>True</crs:ConvertToGrayscale>
   <crs:BlueHue>+50</crs:BlueHue>
   <crs:ToneCurvePV2012><rdf:Seq>
    <rdf:li>0, 30</rdf:li><rdf:li>255, 255</rdf:li>
   </rdf:Seq></crs:ToneCurvePV2012>
  </rdf:Description></crs:Parameters>
 </rdf:Description></crs:Look>
</rdf:Description>"#
}

#[test]
fn creative_look_parameters_do_not_become_global_user_edits() {
    let document = creative_look_document();
    let restored = autoshade::xmp::xmp_to_recipe(document);
    let corrupt =
        document.replace("<crs:Clarity2012>+50</crs:Clarity2012>", "<crs:Clarity2012>--</crs:Clarity2012>");

    assert_eq!(restored.exposure_ev, 0.20);
    assert_eq!((restored.clarity, restored.vibrance), (0.0, 0.0));
    assert!(!restored.convert_to_grayscale);
    assert_eq!(restored.cal_blue_hue, 0.0);
    assert!(restored.tone_curve.is_empty());
    assert!(autoshade::xmp::unparsable_crs_numbers(&corrupt).is_empty());
}

#[test]
fn crossed_look_close_does_not_promote_nested_edits_or_block_merge() {
    let document = r#"<rdf:Description
 xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"
 crs:Exposure2012="+0.35" crs:HasSettings="True">
 <crs:Look><rdf:Description>
  <crs:Clarity2012>+50</crs:Clarity2012>
 </rdf:Description></crs:Foo>
</rdf:Description>"#;

    let imported = autoshade::xmp::xmp_to_recipe(document);

    assert_eq!(imported.exposure_ev, 0.35);
    assert_eq!(imported.clarity, 0.0);
    assert!(
        autoshade::xmp::merge_recipe_into_xmp(document, &EditRecipe::default()).is_some()
    );
}

#[test]
fn top_level_clarity_imports_in_attribute_and_element_forms_beside_a_creative_look() {
    for own in [
        r#"crs:Exposure2012="+0.20" crs:Clarity2012="+12""#,
        r#"crs:Exposure2012="+0.20">
 <crs:Clarity2012>+12</crs:Clarity2012"#,
    ] {
        let document =
            creative_look_document().replace(r#"crs:Exposure2012="+0.20""#, own);

        assert_eq!(autoshade::xmp::xmp_to_recipe(&document).clarity, 12.0);
    }
}

#[test]
fn an_unclosed_description_falls_back_without_losing_top_level_attributes() {
    let document = r#"<rdf:Description
 xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"
 crs:Exposure2012="+0.50">"#;

    assert_eq!(autoshade::xmp::xmp_to_recipe(document).exposure_ev, 0.50);
}

#[test]
fn straighten_without_crop_uses_a_full_frame_carrier_and_round_trips_without_crop() {
    let recipe = EditRecipe {
        straighten_deg: 2.5,
        ..EditRecipe::default()
    };
    let document = without_recipe_payload(autoshade::xmp::recipe_to_xmp(&recipe));
    let restored = autoshade::xmp::xmp_to_recipe(&document);

    assert!(document.contains(r#"crs:HasCrop="True""#));
    assert!(document.contains(r#"crs:CropAngle="-2.500000""#));
    assert!(restored.crop.is_none());
    assert_eq!(restored.straighten_deg, 2.5);
}

#[test]
fn forbidden_xml_controls_are_removed_from_public_mask_names() {
    let recipe = EditRecipe {
        masks: vec![LocalAdjustment {
            name: "sky\u{0}\u{7}".to_string(),
            ..LocalAdjustment::default()
        }],
        ..EditRecipe::default()
    };
    let document = without_recipe_payload(autoshade::xmp::recipe_to_xmp(&recipe));
    let restored = autoshade::xmp::xmp_to_recipe(&document);

    assert!(!document.contains('\u{0}') && !document.contains('\u{7}'));
    assert_eq!(restored.masks[0].name, "sky");
}

fn rotated_radial_recipe(angle: f32) -> EditRecipe {
    EditRecipe {
        masks: vec![LocalAdjustment {
            name: "tilted".to_string(),
            mask: MaskGeometry::Radial {
                top: 0.3,
                left: 0.35,
                bottom: 0.7,
                right: 0.65,
                feather: 0.5,
                roundness: 0.0,
                flipped: false,
                angle,
                midpoint: 50.0,
                mask_version: 2,
            },
            ..LocalAdjustment::default()
        }],
        ..EditRecipe::default()
    }
}

#[test]
fn export_rotation_loss_carries_a_signed_rounded_angle_or_plain_fallback() {
    for (angle, expected) in [(37.412506, 37), (-12.0, -12), (0.4, 0)] {
        let losses = autoshade::xmp::mask_export_losses(&rotated_radial_recipe(angle));

        assert_eq!(
            losses
                .iter()
                .map(|loss| (loss.name.as_str(), loss.reason))
                .collect::<Vec<_>>(),
            [("tilted", MaskLossReason::Rotation(expected))]
        );
    }
}

fn frameless_radial_document(angle: &str) -> String {
    format!(
        r#"<rdf:Description xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/">
<crs:MaskGroupBasedCorrections><rdf:Seq>
 <rdf:li><rdf:Description crs:What="Correction" crs:CorrectionName="Radial 1">
  <crs:CorrectionMasks><rdf:Seq>
   <rdf:li crs:What="Mask/CircularGradient" crs:Top="0.3" crs:Left="0.35"
    crs:Bottom="0.7" crs:Right="0.65" crs:Feather="50" crs:Roundness="0"
    crs:Flipped="false" crs:Angle="{angle}"/>
  </rdf:Seq></crs:CorrectionMasks>
 </rdf:Description></rdf:li>
</rdf:Seq></crs:MaskGroupBasedCorrections>
</rdf:Description>"#
    )
}

#[test]
fn import_rotation_loss_carries_a_signed_angle_and_distinguishes_unrotated() {
    for (angle, expected) in [
        ("-43.945287", Some(MaskImportReason::Rotation(-44))),
        ("oblique", Some(MaskImportReason::Rotation(0))),
        ("0", None),
    ] {
        let losses = autoshade::xmp::import_losses(&frameless_radial_document(angle));

        assert_eq!(
            losses
                .iter()
                .map(|loss| (loss.name.as_str(), loss.reason))
                .collect::<Vec<_>>(),
            expected.map_or_else(Vec::new, |reason| vec![("Radial 1", reason)])
        );
    }
}

fn lightroom_radial_inversion_document(flipped: bool, inverted: bool) -> String {
    format!(
        r#"<rdf:Description xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/">
<crs:MaskGroupBasedCorrections><rdf:Seq>
 <rdf:li><rdf:Description crs:What="Correction" crs:CorrectionName="Radial 1">
  <crs:CorrectionMasks><rdf:Seq>
   <rdf:li crs:What="Mask/CircularGradient" crs:MaskActive="true"
    crs:MaskBlendMode="0" crs:MaskValue="1" crs:MaskInverted="{inverted}"
    crs:Top="0.3" crs:Left="0.35" crs:Bottom="0.7" crs:Right="0.65"
    crs:Feather="50" crs:Roundness="0" crs:Flipped="{flipped}" crs:Angle="0"/>
  </rdf:Seq></crs:CorrectionMasks>
 </rdf:Description></rdf:li>
</rdf:Seq></crs:MaskGroupBasedCorrections>
</rdf:Description>"#
    )
}

#[test]
fn lightroom_radial_inversion_uses_one_bit_and_rewrites_its_complement() {
    for (flipped, inverted) in [(true, false), (false, true), (true, true), (false, false)] {
        let input = lightroom_radial_inversion_document(flipped, inverted);
        let restored = autoshade::xmp::xmp_to_recipe(&input);
        let MaskGeometry::Radial {
            flipped: geometry_flipped,
            ..
        } = restored.masks[0].mask
        else {
            panic!("Lightroom radial imports as a radial");
        };

        assert_eq!(
            (
                geometry_flipped,
                restored.masks[0].inverted,
                restored.masks[0].net_inverted(),
            ),
            (false, inverted, inverted),
            "Flipped={flipped} MaskInverted={inverted}"
        );

        let output = without_recipe_payload(autoshade::xmp::recipe_to_xmp(&restored));
        let expected_net = if inverted { "true" } else { "false" };
        let expected_complement = if inverted { "false" } else { "true" };
        assert_eq!(
            (
                xmp_public_value(&output, "crs:MaskInverted"),
                xmp_public_value(&output, "crs:Flipped"),
            ),
            (Some(expected_net), Some(expected_complement))
        );
        assert_eq!(
            autoshade::xmp::xmp_to_recipe(&output).masks[0].net_inverted(),
            inverted
        );
    }
}

#[test]
fn engine_radial_flip_and_inversion_export_as_one_lightroom_net_bit() {
    for (flipped, inverted) in [(true, false), (false, true), (true, true), (false, false)] {
        let mut recipe = rotated_radial_recipe(0.0);
        let MaskGeometry::Radial {
            flipped: recipe_flipped,
            ..
        } = &mut recipe.masks[0].mask
        else {
            panic!("fixture is radial");
        };
        *recipe_flipped = flipped;
        recipe.masks[0].inverted = inverted;
        let net = flipped ^ inverted;

        let output = without_recipe_payload(autoshade::xmp::recipe_to_xmp(&recipe));
        assert_eq!(
            (
                xmp_public_value(&output, "crs:MaskInverted"),
                xmp_public_value(&output, "crs:Flipped"),
            ),
            (
                Some(if net { "true" } else { "false" }),
                Some(if net { "false" } else { "true" }),
            ),
            "flipped={flipped} inverted={inverted}"
        );
        assert_eq!(
            autoshade::xmp::xmp_to_recipe(&output).masks[0].net_inverted(),
            net
        );
    }
}

fn lightroom_gradient_document(blend: &str) -> String {
    format!(
        r#"<x:xmpmeta xmlns:x="adobe:ns:meta/" x:xmptk="Adobe XMP Core">
<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
<rdf:Description xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/">
 <crs:MaskGroupBasedCorrections><rdf:Seq>
  <rdf:li><rdf:Description crs:What="Correction" crs:CorrectionName="Gradient 1">
   <crs:CorrectionMasks><rdf:Seq>
    <rdf:li crs:What="Mask/Gradient" crs:MaskActive="true"
     crs:MaskBlendMode="{blend}" crs:MaskInverted="false" crs:MaskValue="1"
     crs:ZeroX="0.5" crs:ZeroY="0.8" crs:FullX="0.5" crs:FullY="0.2"/>
   </rdf:Seq></crs:CorrectionMasks>
  </rdf:Description></rdf:li>
 </rdf:Seq></crs:MaskGroupBasedCorrections>
</rdf:Description></rdf:RDF></x:xmpmeta>"#
    )
}

#[test]
fn plain_lightroom_gradient_imports_without_false_loss() {
    let document = lightroom_gradient_document("0");

    assert_eq!(autoshade::xmp::xmp_to_recipe(&document).masks.len(), 1);
    assert_eq!(autoshade::xmp::unsupported_corrections(&document), 0);
    assert!(autoshade::xmp::import_losses(&document).is_empty());
}

#[test]
fn base_only_nondefault_gradient_imports_shape_with_a_blend_mode_note() {
    let document = lightroom_gradient_document("1");

    assert_eq!(autoshade::xmp::xmp_to_recipe(&document).masks.len(), 1);
    assert_eq!(autoshade::xmp::unsupported_corrections(&document), 0);
    assert_eq!(
        autoshade::xmp::import_losses(&document)
            .iter()
            .map(|loss| (loss.name.as_str(), loss.reason))
            .collect::<Vec<_>>(),
        [("Gradient 1", MaskImportReason::BlendMode)]
    );
}

fn lightroom_radial_value_document(blend: &str, value: &str) -> String {
    format!(
        r#"<x:xmpmeta xmlns:x="adobe:ns:meta/" x:xmptk="Adobe XMP Core">
<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
<rdf:Description xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/">
 <crs:MaskGroupBasedCorrections><rdf:Seq>
  <rdf:li><rdf:Description crs:What="Correction" crs:CorrectionName="Radial 1"
   crs:CorrectionAmount="1" crs:LocalContrast2012="0.43">
   <crs:CorrectionMasks><rdf:Seq>
    <rdf:li crs:What="Mask/CircularGradient" crs:MaskActive="true"
     crs:MaskBlendMode="{blend}" crs:MaskInverted="false" crs:MaskValue="{value}"
     crs:Top="0.114928" crs:Left="0.590368" crs:Bottom="0.802847"
     crs:Right="0.921381" crs:Feather="100" crs:Roundness="0"
     crs:Flipped="true" crs:Angle="0"/>
   </rdf:Seq></crs:CorrectionMasks>
  </rdf:Description></rdf:li>
 </rdf:Seq></crs:MaskGroupBasedCorrections>
</rdf:Description></rdf:RDF></x:xmpmeta>"#
    )
}

fn lightroom_radial_component(blend: &str, value: &str) -> String {
    lightroom_radial_component_at(blend, value, "0")
}

fn lightroom_radial_component_at(blend: &str, value: &str, angle: &str) -> String {
    format!(
        r#"<rdf:li crs:What="Mask/CircularGradient" crs:MaskActive="true"
 crs:MaskBlendMode="{blend}" crs:MaskInverted="false" crs:MaskValue="{value}"
 crs:Top="0.1" crs:Left="0.2" crs:Bottom="0.8" crs:Right="0.9"
 crs:Feather="100" crs:Roundness="0" crs:Flipped="true" crs:Angle="{angle}"/>"#
    )
}

fn lightroom_linear_component(blend: &str, value: &str) -> String {
    format!(
        r#"<rdf:li crs:What="Mask/Gradient" crs:MaskActive="true"
 crs:MaskBlendMode="{blend}" crs:MaskInverted="false" crs:MaskValue="{value}"
 crs:ZeroX="0.5" crs:ZeroY="0.8" crs:FullX="0.5" crs:FullY="0.2"/>"#
    )
}

fn lightroom_component_stack_document(components: &[String]) -> String {
    format!(
        r#"<rdf:Description xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/">
 <crs:MaskGroupBasedCorrections><rdf:Seq>
  <rdf:li><rdf:Description crs:What="Correction" crs:CorrectionName="Stacked 1">
   <crs:CorrectionMasks><rdf:Seq>{}</rdf:Seq></crs:CorrectionMasks>
  </rdf:Description></rdf:li>
 </rdf:Seq></crs:MaskGroupBasedCorrections>
</rdf:Description>"#,
        components.concat()
    )
}

fn lightroom_local_curve_element(name: &str, points: &[(u8, u8)]) -> String {
    let points: String = points
        .iter()
        .map(|(input, output)| format!("<rdf:li>{input},{output}</rdf:li>"))
        .collect();
    format!("<crs:{name}><rdf:Seq>{points}</rdf:Seq></crs:{name}>")
}

fn lightroom_local_curve_document(curves: &str) -> String {
    format!(
        r#"<rdf:Description xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/">
 <crs:MaskGroupBasedCorrections><rdf:Seq>
  <rdf:li><rdf:Description crs:What="Correction" crs:CorrectionName="Radial 1">
   {curves}
   <crs:CorrectionMasks><rdf:Seq>{}</rdf:Seq></crs:CorrectionMasks>
  </rdf:Description></rdf:li>
 </rdf:Seq></crs:MaskGroupBasedCorrections>
</rdf:Description>"#,
        lightroom_radial_component("0", "1")
    )
}

#[test]
fn lightroom_component_stack_uses_the_first_add_as_base_and_keeps_every_shape() {
    let radial = || lightroom_radial_component("0", "1");
    let linear = || lightroom_linear_component("0", "1");
    let subtract_radial = || lightroom_radial_component("1", "0");
    let subtract_linear = || lightroom_linear_component("1", "0");
    for (components, radial_base) in [
        (vec![radial(), radial(), linear()], true),
        (vec![linear(), radial()], false),
        (vec![radial(), subtract_linear()], true),
        (vec![subtract_linear(), radial()], true),
        (vec![subtract_linear(), subtract_radial()], false),
    ] {
        let document = lightroom_component_stack_document(&components);
        let imported = autoshade::xmp::xmp_to_recipe(&document);

        assert_eq!(imported.masks.len(), 1);
        assert_eq!(
            matches!(imported.masks[0].mask, MaskGeometry::Radial { .. }),
            radial_base
        );
        assert_eq!(imported.masks[0].components.len(), components.len() - 1);
        assert!(!autoshade::xmp::import_losses(&document)
            .iter()
            .any(|loss| loss.reason == MaskImportReason::MultiComponent));
    }
}

#[test]
fn stacked_radial_rotation_is_disclosed_without_false_composition_losses() {
    for components in [
        vec![
            lightroom_linear_component("0", "1"),
            lightroom_radial_component_at("0", "1", "37.412506"),
        ],
        vec![
            lightroom_radial_component_at("0", "1", "37.412506"),
            lightroom_linear_component("0", "1"),
        ],
    ] {
        let document = lightroom_component_stack_document(&components);
        let reasons = autoshade::xmp::import_losses(&document)
            .into_iter()
            .map(|loss| loss.reason)
            .collect::<Vec<_>>();

        assert!(reasons.contains(&MaskImportReason::Rotation(37)));
        assert!(!reasons.contains(&MaskImportReason::MultiComponent));
    }

    let subtract = lightroom_component_stack_document(&[
        lightroom_radial_component("0", "1"),
        lightroom_linear_component("1", "0"),
    ]);
    assert!(!autoshade::xmp::import_losses(&subtract)
        .iter()
        .any(|loss| loss.reason == MaskImportReason::BlendMode));
}

#[test]
fn every_import_loss_reason_reaches_grouped_public_disclosure() {
    let losses = vec![
        MaskImportLoss {
            name: "unrepresentable".to_string(),
            reason: MaskImportReason::Unrepresentable,
        },
        MaskImportLoss {
            name: "out-of-model".to_string(),
            reason: MaskImportReason::OutOfModel,
        },
        MaskImportLoss {
            name: "rotation-a".to_string(),
            reason: MaskImportReason::Rotation(37),
        },
        MaskImportLoss {
            name: "rotation-b".to_string(),
            reason: MaskImportReason::Rotation(-44),
        },
        MaskImportLoss {
            name: "blend".to_string(),
            reason: MaskImportReason::BlendMode,
        },
        MaskImportLoss {
            name: "multi".to_string(),
            reason: MaskImportReason::MultiComponent,
        },
        MaskImportLoss {
            name: "brush".to_string(),
            reason: MaskImportReason::BrushRendered,
        },
        MaskImportLoss {
            name: "ai-recomputed".to_string(),
            reason: MaskImportReason::AiMaskRecomputed,
        },
        MaskImportLoss {
            name: "ai-unresolved".to_string(),
            reason: MaskImportReason::AiMaskUnresolved,
        },
        MaskImportLoss {
            name: "range".to_string(),
            reason: MaskImportReason::ForeignRangeMask,
        },
        MaskImportLoss {
            name: "curve".to_string(),
            reason: MaskImportReason::LocalCurve,
        },
        MaskImportLoss {
            name: "curve-refine".to_string(),
            reason: MaskImportReason::CurveRefineSaturation,
        },
        MaskImportLoss {
            name: "inert-a".to_string(),
            reason: MaskImportReason::InertLocal("LocalGrain"),
        },
        MaskImportLoss {
            name: "inert-b".to_string(),
            reason: MaskImportReason::InertLocal("LocalMoire"),
        },
        MaskImportLoss {
            name: "unknown".to_string(),
            reason: MaskImportReason::UnknownLocalKey,
        },
    ];

    let line = autoshade::xmp::describe_import_losses(3, &losses).unwrap();

    assert!(line.contains("imported 3 Lightroom mask(s)"));
    for loss in &losses {
        assert!(line.contains(loss.reason.en()), "{:?}", loss.reason);
        assert!(line.contains(&loss.name), "{}", loss.name);
    }
    assert_eq!(
        line.matches(MaskImportReason::Rotation(0).en()).count(),
        1
    );
    assert_eq!(
        line.matches(MaskImportReason::InertLocal("").en()).count(),
        1
    );
}

#[test]
fn sparse_lightroom_local_curves_round_trip_without_inventing_channels() {
    let curves = format!(
        "{}{}",
        lightroom_local_curve_element("RedCurve", &[(0, 0), (239, 255)]),
        lightroom_local_curve_element("GreenCurve", &[(0, 12), (128, 140), (255, 255)])
    );
    let imported = autoshade::xmp::xmp_to_recipe(&lightroom_local_curve_document(&curves));
    assert_eq!(imported.masks.len(), 1);
    assert_eq!(
        imported.masks[0].red_curve,
        vec![
            CurvePoint {
                input: 0,
                output: 0,
            },
            CurvePoint {
                input: 239,
                output: 255,
            },
        ]
    );
    assert_eq!(imported.masks[0].green_curve.len(), 3);
    assert!(imported.masks[0].main_curve.is_empty());
    assert!(imported.masks[0].blue_curve.is_empty());

    let reopened = autoshade::xmp::xmp_to_recipe(&autoshade::xmp::recipe_to_xmp(&imported));
    assert_eq!(reopened.masks[0].red_curve, imported.masks[0].red_curve);
    assert_eq!(
        reopened.masks[0].green_curve,
        imported.masks[0].green_curve
    );
    assert!(reopened.masks[0].main_curve.is_empty());
    assert!(reopened.masks[0].blue_curve.is_empty());
}

#[test]
fn local_and_global_curve_spelling_and_order_match_lightroom() {
    let document = autoshade::xmp::recipe_to_xmp(&EditRecipe {
        tone_curve: vec![
            CurvePoint {
                input: 10,
                output: 20,
            },
            CurvePoint {
                input: 255,
                output: 255,
            },
        ],
        masks: vec![LocalAdjustment {
            amount: 1.0,
            main_curve: vec![
                CurvePoint {
                    input: 32,
                    output: 48,
                },
                CurvePoint {
                    input: 255,
                    output: 255,
                },
            ],
            ..LocalAdjustment::default()
        }],
        ..EditRecipe::default()
    });

    assert!(document.contains("<rdf:li>32,48</rdf:li>"));
    assert!(!document.contains("<rdf:li>32, 48</rdf:li>"));
    assert!(document.contains("<rdf:li>10, 20</rdf:li>"));
    assert!(!document.contains("<rdf:li>10,20</rdf:li>"));
    assert!(document.contains("<crs:MainCurve>"));
    let correction = document
        .split_once(r#"crs:LocalCurveRefineSaturation="100">"#)
        .unwrap()
        .1;
    assert!(
        correction.find("<crs:MainCurve>").unwrap()
            < correction.find("<crs:CorrectionMasks>").unwrap()
    );
}

#[test]
fn channel_curve_without_composite_exports_identity_and_round_trips() {
    let recipe = EditRecipe {
        blue_curve: vec![
            CurvePoint {
                input: 0,
                output: 0,
            },
            CurvePoint {
                input: 128,
                output: 150,
            },
            CurvePoint {
                input: 255,
                output: 255,
            },
        ],
        ..EditRecipe::default()
    };
    let document = autoshade::xmp::recipe_to_xmp(&recipe);

    let composite = document.find("<crs:ToneCurvePV2012>").unwrap();
    let blue = document.find("<crs:ToneCurvePV2012Blue>").unwrap();
    assert!(composite < blue);
    assert!(document.contains(
        "<rdf:li>0, 0</rdf:li>\n     <rdf:li>255, 255</rdf:li>"
    ));
    let reopened = autoshade::xmp::xmp_to_recipe(&document);
    assert!(reopened.tone_curve.is_empty());
    assert_eq!(reopened.blue_curve, recipe.blue_curve);
    assert!(!autoshade::xmp::recipe_to_xmp(&EditRecipe::default())
        .contains("<crs:ToneCurvePV2012>"));
}

#[test]
fn local_curve_export_stays_sparse_for_curved_and_plain_masks() {
    let curved = autoshade::xmp::recipe_to_xmp(&EditRecipe {
        masks: vec![LocalAdjustment {
            amount: 1.0,
            red_curve: vec![
                CurvePoint {
                    input: 0,
                    output: 0,
                },
                CurvePoint {
                    input: 128,
                    output: 160,
                },
            ],
            ..LocalAdjustment::default()
        }],
        ..EditRecipe::default()
    });
    assert!(curved.contains("<crs:RedCurve>"));
    for absent in ["MainCurve", "GreenCurve", "BlueCurve"] {
        assert!(!curved.contains(&format!("<crs:{absent}>")));
    }

    let plain = autoshade::xmp::recipe_to_xmp(&EditRecipe {
        masks: vec![LocalAdjustment {
            amount: 1.0,
            ..LocalAdjustment::default()
        }],
        ..EditRecipe::default()
    });
    for absent in ["MainCurve", "RedCurve", "GreenCurve", "BlueCurve"] {
        assert!(!plain.contains(&format!("<crs:{absent}>")));
    }
}

#[test]
fn readable_local_curve_imports_without_false_loss() {
    let curves =
        lightroom_local_curve_element("MainCurve", &[(0, 0), (128, 96), (255, 255)]);
    let document = lightroom_local_curve_document(&curves);

    assert!(autoshade::xmp::import_losses(&document).is_empty());
    assert_eq!(autoshade::xmp::unsupported_corrections(&document), 0);
    assert_eq!(
        autoshade::xmp::xmp_to_recipe(&document).masks[0]
            .main_curve
            .len(),
        3
    );
}

#[test]
fn unreadable_local_curve_keeps_the_mask_and_names_the_loss() {
    let curves =
        lightroom_local_curve_element("BlueCurve", &[(0, 0), (255, 255)])
            .replace("<rdf:li>255,255</rdf:li>", "<rdf:li>999,-5</rdf:li>");
    let document = lightroom_local_curve_document(&curves);
    let imported = autoshade::xmp::xmp_to_recipe(&document);

    assert_eq!(imported.masks.len(), 1);
    assert!(imported.masks[0].blue_curve.is_empty());
    assert_eq!(
        autoshade::xmp::import_losses(&document),
        vec![MaskImportLoss {
            name: "Radial 1".to_string(),
            reason: MaskImportReason::LocalCurve,
        }]
    );
}

fn lightroom_tinted_radial_document(saturation: &str) -> String {
    lightroom_radial_value_document("0", "1").replace(
        r#"crs:CorrectionAmount="1""#,
        &format!(
            r#"crs:CorrectionAmount="1" crs:LocalToningHue="231" crs:LocalToningSaturation="{saturation}""#
        ),
    )
}

#[test]
fn supported_mask_tint_imports_exports_and_round_trips_without_loss() {
    let document = lightroom_tinted_radial_document("0.73");
    let imported = autoshade::xmp::xmp_to_recipe(&document);

    assert_eq!(imported.masks.len(), 1);
    assert_eq!(
        (
            imported.masks[0].toning_hue,
            imported.masks[0].toning_amount,
        ),
        (231.0, 73.0)
    );
    assert!(autoshade::xmp::import_losses(&document).is_empty());

    let exported = autoshade::xmp::recipe_to_xmp(&imported);
    assert!(exported.contains(r#"crs:LocalToningHue="231""#));
    assert!(exported.contains(r#"crs:LocalToningSaturation="0.73""#));
    let reopened = autoshade::xmp::xmp_to_recipe(&exported);
    assert_eq!(
        (
            reopened.masks[0].toning_hue,
            reopened.masks[0].toning_amount,
        ),
        (231.0, 73.0)
    );
}

#[test]
fn mask_tint_strength_outside_the_swatch_range_is_refused() {
    let document = lightroom_tinted_radial_document("1.5");

    assert!(autoshade::xmp::xmp_to_recipe(&document).masks.is_empty());
    assert_eq!(autoshade::xmp::unsupported_corrections(&document), 1);
    assert_eq!(
        autoshade::xmp::import_losses(&document)
            .iter()
            .map(|loss| (loss.name.as_str(), loss.reason))
            .collect::<Vec<_>>(),
        [("Radial 1", MaskImportReason::OutOfModel)]
    );
}

#[test]
fn lightroom_subtract_pair_keeps_radial_geometry_with_a_typed_note() {
    let document = lightroom_radial_value_document("1", "0");
    let restored = autoshade::xmp::xmp_to_recipe(&document);
    assert_eq!(restored.masks.len(), 1);
    let MaskGeometry::Radial {
        top, left, ..
    } = restored.masks[0].mask
    else {
        panic!("subtract pair keeps its radial");
    };

    assert_eq!(autoshade::xmp::unsupported_corrections(&document), 0);
    assert_eq!(
        autoshade::xmp::import_losses(&document)
            .iter()
            .map(|loss| (loss.name.as_str(), loss.reason))
            .collect::<Vec<_>>(),
        [("Radial 1", MaskImportReason::BlendMode)]
    );
    assert!((top as f64 - 0.114928).abs() < 1e-7);
    assert!((left as f64 - 0.590368).abs() < 1e-7);
}

#[test]
fn subtract_mask_value_zero_does_not_mute_the_adjustment() {
    let restored =
        autoshade::xmp::xmp_to_recipe(&lightroom_radial_value_document("1", "0"));

    assert_eq!(
        (restored.masks[0].amount, restored.masks[0].contrast),
        (1.0, 43.0)
    );
}

#[test]
fn default_zero_mask_value_refuses_a_muted_component() {
    let document = lightroom_radial_value_document("0", "0");

    assert!(autoshade::xmp::xmp_to_recipe(&document).masks.is_empty());
    assert_eq!(autoshade::xmp::unsupported_corrections(&document), 1);
}

#[test]
fn partial_mask_value_refuses_default_and_subtract_components() {
    for blend in ["0", "1"] {
        let document = lightroom_radial_value_document(blend, "0.662178");

        assert!(autoshade::xmp::xmp_to_recipe(&document).masks.is_empty());
        assert_eq!(autoshade::xmp::unsupported_corrections(&document), 1);
    }
}

#[test]
fn lightroom_roundness_values_are_carried_without_rescaling() {
    for (spelling, expected) in [("-30", -30.0), ("+45", 45.0), ("1", 1.0), ("0.25", 0.25)] {
        let document =
            lightroom_radial_value_document("0", "1").replace(r#"crs:Roundness="0""#, &format!(r#"crs:Roundness="{spelling}""#));
        let restored = autoshade::xmp::xmp_to_recipe(&document);

        assert_eq!(restored.masks.len(), 1);
        assert!(autoshade::xmp::import_losses(&document).is_empty());
        let MaskGeometry::Radial { roundness, .. } = restored.masks[0].mask else {
            panic!("Roundness={spelling} keeps a radial");
        };
        assert_eq!(roundness, expected);

        let rewritten = without_recipe_payload(autoshade::xmp::recipe_to_xmp(&restored));
        let MaskGeometry::Radial {
            roundness: reread,
            ..
        } = autoshade::xmp::xmp_to_recipe(&rewritten).masks[0].mask
        else {
            panic!("Roundness={spelling} survives the writer");
        };
        assert_eq!(reread, expected);
    }
}

#[test]
fn roundness_past_lightroom_range_refuses_the_radial() {
    let document =
        lightroom_radial_value_document("0", "1").replace(r#"crs:Roundness="0""#, r#"crs:Roundness="101""#);

    assert!(autoshade::xmp::xmp_to_recipe(&document).masks.is_empty());
    assert_eq!(autoshade::xmp::unsupported_corrections(&document), 1);
}

#[test]
fn nonzero_inert_local_keeps_the_mask_and_names_the_unmodelled_slider() {
    let document = lightroom_radial_value_document("0", "1").replace(
        r#"crs:LocalContrast2012="0.43""#,
        r#"crs:LocalContrast2012="0.43" crs:LocalDefringe="0.3""#,
    );
    let restored = autoshade::xmp::xmp_to_recipe(&document);

    assert_eq!(restored.masks.len(), 1);
    assert_eq!(restored.masks[0].contrast, 43.0);
    assert_eq!(
        autoshade::xmp::import_losses(&document)
            .iter()
            .map(|loss| (loss.name.as_str(), loss.reason))
            .collect::<Vec<_>>(),
        [(
            "Radial 1",
            MaskImportReason::InertLocal("LocalDefringe"),
        )]
    );
    assert_eq!(autoshade::xmp::unsupported_corrections(&document), 0);
}

#[test]
fn unknown_local_and_curve_refine_losses_stack_and_name_the_correction() {
    let document = lightroom_radial_value_document("0", "1")
        .replace(r#"crs:CorrectionName="Radial 1""#, r#"crs:CorrectionName="Sky""#)
        .replace(
            r#"crs:LocalContrast2012="0.43""#,
            r#"crs:LocalContrast2012="0.43" crs:LocalWhatsit="0.5" crs:LocalCurveRefineSaturation="80""#,
        );
    let restored = autoshade::xmp::xmp_to_recipe(&document);
    let losses = autoshade::xmp::import_losses(&document);

    assert_eq!(restored.masks.len(), 1);
    assert_eq!(losses.len(), 2);
    assert!(losses.iter().all(|loss| loss.name == "Sky"));
    assert!(losses
        .iter()
        .any(|loss| loss.reason == MaskImportReason::UnknownLocalKey));
    assert!(losses
        .iter()
        .any(|loss| loss.reason == MaskImportReason::CurveRefineSaturation));
}

fn lightroom_bookkeeping_document(corrected_depth: &str, digest_version: &str) -> String {
    lightroom_radial_value_document("0", "1")
        .replace(r#"crs:CorrectionName="Radial 1""#, r#"crs:CorrectionName="Sky""#)
        .replace(
            r#"crs:LocalContrast2012="0.43""#,
            &format!(
                r#"crs:LocalContrast2012="0.43" crs:LocalCorrectedDepth="{corrected_depth}" crs:LocalInputDigest="0000000000000000000000000000002A" crs:LocalInputDigestVersion="{digest_version}""#
            ),
        )
}

#[test]
fn lightroom_digest_ledger_is_not_reported_as_an_unmodelled_slider() {
    let neutral = lightroom_bookkeeping_document("0", "1");
    let another_version = lightroom_bookkeeping_document("0", "2");

    assert_eq!(autoshade::xmp::xmp_to_recipe(&neutral).masks.len(), 1);
    assert!(autoshade::xmp::import_losses(&neutral).is_empty());
    assert!(autoshade::xmp::import_losses(&another_version).is_empty());
}

#[test]
fn nonzero_corrected_depth_is_named_without_dropping_the_mask() {
    let document = lightroom_bookkeeping_document("0.5", "1");

    assert_eq!(autoshade::xmp::xmp_to_recipe(&document).masks.len(), 1);
    assert_eq!(
        autoshade::xmp::import_losses(&document)
            .iter()
            .map(|loss| (loss.name.as_str(), loss.reason))
            .collect::<Vec<_>>(),
        [(
            "Sky",
            MaskImportReason::InertLocal("LocalCorrectedDepth"),
        )]
    );
}

fn xmp_element_block<'a>(document: &'a str, name: &str) -> &'a str {
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    let start = document.find(&open).unwrap();
    let end = document.find(&close).unwrap() + close.len();
    &document[start..end]
}

#[test]
fn untouched_lossy_mask_block_survives_recipe_persistence_and_merge() {
    let document = lightroom_radial_value_document("0", "1").replace(
        r#"crs:Angle="0""#,
        r#"crs:Angle="37.412506" crs:Midpoint="50" crs:Version="2""#,
    );
    let imported = autoshade::xmp::xmp_to_recipe(&document);
    assert_eq!(imported.masks.len(), 1);
    assert_eq!(autoshade::xmp::import_losses(&document).len(), 1);

    let json = serde_json::to_string(&imported).unwrap();
    let reloaded: EditRecipe = serde_json::from_str(&json).unwrap();
    assert_eq!(reloaded.masks, imported.masks);

    let original = xmp_element_block(&document, "crs:MaskGroupBasedCorrections");
    let merged = autoshade::xmp::merge_recipe_into_xmp(&document, &reloaded).unwrap();

    assert!(merged.doc.contains(original));
    assert!(merged.doc.contains(r#"crs:Angle="37.412506" crs:Midpoint="50""#));
    assert!(merged.notes.is_empty());
    assert_eq!(
        merged
            .doc
            .matches("<crs:MaskGroupBasedCorrections>")
            .count(),
        1
    );
}

#[test]
fn clean_lightroom_mask_block_survives_a_legacy_maskless_recipe() {
    let document = lightroom_radial_value_document("0", "1");
    let imported = autoshade::xmp::xmp_to_recipe(&document);
    assert_eq!(imported.masks.len(), 1);
    assert!(autoshade::xmp::import_losses(&document).is_empty());

    let mut value = serde_json::to_value(&imported).unwrap();
    let object = value.as_object_mut().unwrap();
    object.remove("schema_era");
    object.remove("masks");
    let legacy: EditRecipe = serde_json::from_value(value).unwrap();
    assert_eq!((legacy.schema_era, legacy.masks.len()), (0, 0));

    let original = xmp_element_block(&document, "crs:MaskGroupBasedCorrections");
    let merged = autoshade::xmp::merge_recipe_into_xmp(&document, &legacy).unwrap();

    assert!(merged.doc.contains(original));
    assert_eq!(
        merged
            .doc
            .matches("<crs:MaskGroupBasedCorrections>")
            .count(),
        1
    );
    assert!(merged.notes.is_empty());
}

#[test]
fn edited_clean_mask_replacement_names_the_corrections_it_replaced() {
    let document = lightroom_radial_value_document("0", "1");
    assert!(autoshade::xmp::import_losses(&document).is_empty());
    let mut edited = autoshade::xmp::xmp_to_recipe(&document);
    edited.masks[0].exposure_ev = 1.25;

    let merged = autoshade::xmp::merge_recipe_into_xmp(&document, &edited).unwrap();

    assert_eq!(merged.notes.len(), 1);
    assert!(
        merged.notes[0].contains("1 correction(s)")
            && !merged.notes[0].contains("0 thing(s)")
            && merged.notes[0].contains("1 edited mask(s)")
    );
}

#[test]
fn edited_lossy_mask_replaces_the_original_block_with_disclosure() {
    let document = lightroom_radial_value_document("0", "1").replace(
        r#"crs:Angle="0""#,
        r#"crs:Angle="37.412506" crs:Midpoint="50" crs:Version="2""#,
    );
    let mut edited = autoshade::xmp::xmp_to_recipe(&document);
    edited.masks[0].exposure_ev = 1.25;

    let merged = autoshade::xmp::merge_recipe_into_xmp(&document, &edited).unwrap();

    assert!(!merged.doc.contains(r#"crs:Angle="37.412506""#));
    assert_eq!(merged.notes.len(), 1);
    assert!(
        merged.notes[0].contains("1 thing(s)")
            && merged.notes[0].contains("1 edited mask(s)")
    );
    assert_eq!(
        autoshade::xmp::xmp_to_recipe(&merged.doc).masks[0].exposure_ev,
        1.25
    );
}

fn lightroom_legacy_globals_document() -> &'static str {
    r#"<rdf:Description xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"
 crs:Exposure2012="+0.35" crs:Texture="-20"
 crs:SharpenRadius="+1.0" crs:SharpenDetail="25" crs:SharpenEdgeMasking="13"
 crs:LuminanceNoiseReductionDetail="45" crs:LuminanceNoiseReductionContrast="17"
 crs:ColorNoiseReduction="25" crs:ColorNoiseReductionDetail="47"
 crs:ColorNoiseReductionSmoothness="53"
 crs:ChromaticAberrationR="7" crs:ChromaticAberrationB="-8"
 crs:AutoLateralCA="1"
 crs:GrainAmount="30" crs:GrainSize="25" crs:GrainFrequency="50"
 crs:DefringePurpleAmount="3" crs:DefringePurpleHueLo="30"
 crs:DefringePurpleHueHi="70" crs:DefringeGreenAmount="0"
 crs:DefringeGreenHueLo="40" crs:DefringeGreenHueHi="60"
 crs:PostCropVignetteAmount="-17" crs:PostCropVignetteMidpoint="50"
 crs:PostCropVignetteFeather="61" crs:PostCropVignetteRoundness="-11"
 crs:PostCropVignetteStyle="2" crs:PostCropVignetteHighlightContrast="22"
 crs:HasSettings="True"/>"#
}

#[test]
fn legacy_recipe_missing_modern_fields_preserves_lightroom_globals() {
    let legacy: EditRecipe = serde_json::from_str(r#"{"exposure_ev":0.35}"#).unwrap();
    assert_eq!((legacy.schema_era, legacy.texture, legacy.grain), (0, 0.0, 0.0));

    let merged =
        autoshade::xmp::merge_recipe_into_xmp(lightroom_legacy_globals_document(), &legacy)
            .unwrap();

    for spelling in [
        r#"crs:Texture="-20""#,
        r#"crs:SharpenRadius="+1.0""#,
        r#"crs:SharpenDetail="25""#,
        r#"crs:SharpenEdgeMasking="13""#,
        r#"crs:LuminanceNoiseReductionDetail="45""#,
        r#"crs:LuminanceNoiseReductionContrast="17""#,
        r#"crs:ColorNoiseReduction="25""#,
        r#"crs:ColorNoiseReductionDetail="47""#,
        r#"crs:ColorNoiseReductionSmoothness="53""#,
        r#"crs:ChromaticAberrationR="7""#,
        r#"crs:ChromaticAberrationB="-8""#,
        r#"crs:AutoLateralCA="1""#,
        r#"crs:PostCropVignetteAmount="-17""#,
        r#"crs:PostCropVignetteMidpoint="50""#,
        r#"crs:PostCropVignetteFeather="61""#,
        r#"crs:PostCropVignetteRoundness="-11""#,
        r#"crs:PostCropVignetteStyle="2""#,
        r#"crs:PostCropVignetteHighlightContrast="22""#,
        r#"crs:GrainAmount="30""#,
        r#"crs:GrainSize="25""#,
        r#"crs:GrainFrequency="50""#,
        r#"crs:DefringePurpleAmount="3""#,
        r#"crs:DefringePurpleHueLo="30""#,
        r#"crs:DefringePurpleHueHi="70""#,
        r#"crs:DefringeGreenAmount="0""#,
        r#"crs:DefringeGreenHueLo="40""#,
        r#"crs:DefringeGreenHueHi="60""#,
    ] {
        assert!(merged.doc.contains(spelling), "{spelling}");
    }
}

#[test]
fn current_recipe_owns_modern_globals_it_explicitly_clears() {
    let mut current = autoshade::xmp::xmp_to_recipe(lightroom_legacy_globals_document());
    assert_eq!(current.schema_era, autoshade::recipe::SCHEMA_ERA);
    current.texture = 0.0;
    current.grain = 0.0;

    let merged =
        autoshade::xmp::merge_recipe_into_xmp(lightroom_legacy_globals_document(), &current)
            .unwrap();

    assert!(merged.doc.contains(r#"crs:Texture="0""#));
    assert!(!merged.doc.contains("crs:GrainAmount="));
    assert_eq!(autoshade::xmp::xmp_to_recipe(&merged.doc).texture, 0.0);
}

#[test]
fn edited_legacy_key_leaves_the_era_gate_without_releasing_its_neighbors() {
    let legacy: EditRecipe =
        serde_json::from_str(r#"{"exposure_ev":0.35,"texture":20.0}"#).unwrap();
    let merged =
        autoshade::xmp::merge_recipe_into_xmp(lightroom_legacy_globals_document(), &legacy)
            .unwrap();

    assert!(merged.doc.contains(r#"crs:Texture="+20""#));
    assert_eq!(merged.doc.matches("crs:Texture=").count(), 1);
    assert!(merged.doc.contains(r#"crs:GrainAmount="30""#));
}

#[test]
fn edited_legacy_defringe_rewrites_the_whole_block_once() {
    let legacy: EditRecipe =
        serde_json::from_str(r#"{"exposure_ev":0.35,"defringe_purple":5.0}"#).unwrap();
    let merged =
        autoshade::xmp::merge_recipe_into_xmp(lightroom_legacy_globals_document(), &legacy)
            .unwrap();

    for key in [
        "DefringePurpleAmount",
        "DefringePurpleHueLo",
        "DefringePurpleHueHi",
        "DefringeGreenAmount",
        "DefringeGreenHueLo",
        "DefringeGreenHueHi",
    ] {
        assert_eq!(merged.doc.matches(&format!("crs:{key}=")).count(), 1, "{key}");
    }
    assert!(merged.doc.contains(r#"crs:DefringePurpleAmount="5""#));
}

fn lightroom_v150_globals_document() -> &'static str {
    r#"<rdf:Description xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"
 crs:Exposure2012="+0.35" crs:Texture="-20"
 crs:CameraProfile="Adobe Standard"
 crs:LensProfileSetup="LensDefaults"
 crs:LensProfileName="Adobe Synthetic Lens"
 crs:LensProfileFilename="Synthetic.lcp"
 crs:LensProfileDistortionScale="65" crs:LensProfileVignettingScale="75"
 crs:PerspectiveVertical="11" crs:PerspectiveHorizontal="-12"
 crs:PerspectiveRotate="+0.9" crs:PerspectiveScale="90"
 crs:PerspectiveAspect="13" crs:PerspectiveX="+1.25" crs:PerspectiveY="-2.50"
 crs:PerspectiveUpright="3" crs:CropConstrainToWarp="1"
 crs:ParametricShadows="-10" crs:ParametricDarks="+20"
 crs:ParametricLights="-30" crs:ParametricHighlights="+40"
 crs:ParametricShadowSplit="20" crs:ParametricMidtoneSplit="55"
 crs:ParametricHighlightSplit="85"
 crs:ShadowTint="+1" crs:RedHue="+2" crs:RedSaturation="+3"
 crs:GreenHue="+4" crs:GreenSaturation="+5"
 crs:BlueHue="+6" crs:BlueSaturation="+7"
 crs:ConvertToGrayscale="True"
 crs:GrayMixerRed="+11" crs:GrayMixerOrange="+12"
 crs:GrayMixerYellow="+13" crs:GrayMixerGreen="+14"
 crs:GrayMixerAqua="+15" crs:GrayMixerBlue="+16"
 crs:GrayMixerPurple="+17" crs:GrayMixerMagenta="+18"
 crs:HDREditMode="1" crs:HDRMaxValue="+1.00"
 crs:SDRBlend="+10" crs:SDRBrightness="+11" crs:SDRContrast="+12"
 crs:SDRHighlights="-13" crs:SDRShadows="+14"
 crs:SDRWhites="+15" crs:SDRClarity="+16"
 crs:HasSettings="True"/>"#
}

const PARAMETRIC_LIGHTROOM_SPELLINGS: [&str; 7] = [
    r#"crs:ParametricShadows="-10""#,
    r#"crs:ParametricDarks="+20""#,
    r#"crs:ParametricLights="-30""#,
    r#"crs:ParametricHighlights="+40""#,
    r#"crs:ParametricShadowSplit="20""#,
    r#"crs:ParametricMidtoneSplit="55""#,
    r#"crs:ParametricHighlightSplit="85""#,
];

const CALIBRATION_LIGHTROOM_SPELLINGS: [&str; 7] = [
    r#"crs:ShadowTint="+1""#,
    r#"crs:RedHue="+2""#,
    r#"crs:RedSaturation="+3""#,
    r#"crs:GreenHue="+4""#,
    r#"crs:GreenSaturation="+5""#,
    r#"crs:BlueHue="+6""#,
    r#"crs:BlueSaturation="+7""#,
];

const GRAY_MIXER_LIGHTROOM_SPELLINGS: [&str; 8] = [
    r#"crs:GrayMixerRed="+11""#,
    r#"crs:GrayMixerOrange="+12""#,
    r#"crs:GrayMixerYellow="+13""#,
    r#"crs:GrayMixerGreen="+14""#,
    r#"crs:GrayMixerAqua="+15""#,
    r#"crs:GrayMixerBlue="+16""#,
    r#"crs:GrayMixerPurple="+17""#,
    r#"crs:GrayMixerMagenta="+18""#,
];

#[test]
fn lightroom_parametric_curve_merges_once_and_reopens_unchanged() {
    let imported = autoshade::xmp::xmp_to_recipe(lightroom_v150_globals_document());
    assert_eq!(
        imported.parametric_regions(),
        Some([-10.0, 20.0, -30.0, 40.0])
    );
    assert_eq!(imported.parametric_splits(), [20.0, 55.0, 85.0]);

    let merged =
        autoshade::xmp::merge_recipe_into_xmp(lightroom_v150_globals_document(), &imported)
            .unwrap();
    for spelling in PARAMETRIC_LIGHTROOM_SPELLINGS {
        assert!(merged.doc.contains(spelling), "{spelling}");
        let key = &spelling[..spelling.find('=').unwrap() + 1];
        assert_eq!(merged.doc.matches(key).count(), 1, "{key}");
    }
    let reopened = autoshade::xmp::xmp_to_recipe(&merged.doc);
    assert_eq!(
        (
            reopened.parametric_regions(),
            reopened.parametric_splits(),
        ),
        (
            imported.parametric_regions(),
            imported.parametric_splits(),
        )
    );
}

#[test]
fn moving_only_a_parametric_split_exports_the_whole_curve_block() {
    let document = autoshade::xmp::recipe_to_xmp(&EditRecipe {
        param_midtone_split: 60.0,
        ..EditRecipe::default()
    });

    for key in [
        "ParametricShadows",
        "ParametricDarks",
        "ParametricLights",
        "ParametricHighlights",
        "ParametricShadowSplit",
        "ParametricMidtoneSplit",
        "ParametricHighlightSplit",
    ] {
        assert_eq!(document.matches(&format!("crs:{key}=")).count(), 1, "{key}");
    }
    assert!(document.contains(r#"crs:ParametricMidtoneSplit="60""#));
    assert!(document.contains(r#"crs:ParametricDarks="0""#));
}

#[test]
fn neutral_recipe_does_not_invent_a_parametric_curve() {
    let document = autoshade::xmp::recipe_to_xmp(&EditRecipe::default());

    assert!(!document.contains("crs:Parametric"));
    assert!(autoshade::xmp::xmp_to_recipe(&document).is_noop());
}

#[test]
fn lightroom_calibration_and_black_and_white_merge_once_and_reopen_unchanged() {
    let imported = autoshade::xmp::xmp_to_recipe(lightroom_v150_globals_document());
    assert_eq!(
        imported.calibration(),
        Some([1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0])
    );
    assert!(imported.convert_to_grayscale);
    assert_eq!(
        imported.gray_mixer(),
        [11.0, 12.0, 13.0, 14.0, 15.0, 16.0, 17.0, 18.0]
    );

    let merged =
        autoshade::xmp::merge_recipe_into_xmp(lightroom_v150_globals_document(), &imported)
            .unwrap();
    for spelling in CALIBRATION_LIGHTROOM_SPELLINGS
        .into_iter()
        .chain(GRAY_MIXER_LIGHTROOM_SPELLINGS)
        .chain([r#"crs:ConvertToGrayscale="True""#])
    {
        assert!(merged.doc.contains(spelling), "{spelling}");
        let key = &spelling[..spelling.find('=').unwrap() + 1];
        assert_eq!(merged.doc.matches(key).count(), 1, "{key}");
    }
    let reopened = autoshade::xmp::xmp_to_recipe(&merged.doc);
    assert_eq!(
        (
            reopened.calibration(),
            reopened.convert_to_grayscale,
            reopened.gray_mixer(),
        ),
        (
            imported.calibration(),
            imported.convert_to_grayscale,
            imported.gray_mixer(),
        )
    );
}

#[test]
fn one_calibration_edit_exports_the_whole_block_without_black_and_white() {
    let document = autoshade::xmp::recipe_to_xmp(&EditRecipe {
        cal_green_sat: 37.0,
        ..EditRecipe::default()
    });

    for key in [
        "ShadowTint",
        "RedHue",
        "RedSaturation",
        "GreenHue",
        "GreenSaturation",
        "BlueHue",
        "BlueSaturation",
    ] {
        assert_eq!(document.matches(&format!("crs:{key}=")).count(), 1, "{key}");
    }
    assert!(document.contains(r#"crs:GreenSaturation="+37""#));
    assert!(document.contains(r#"crs:ShadowTint="0""#));
    assert!(!document.contains("crs:GrayMixer"));
    assert!(!document.contains("crs:ConvertToGrayscale"));
}

#[test]
fn black_and_white_switch_exports_the_whole_neutral_mixer_without_calibration() {
    let document = autoshade::xmp::recipe_to_xmp(&EditRecipe {
        convert_to_grayscale: true,
        ..EditRecipe::default()
    });

    assert!(document.contains(r#"crs:ConvertToGrayscale="True""#));
    for band in [
        "Red", "Orange", "Yellow", "Green", "Aqua", "Blue", "Purple", "Magenta",
    ] {
        assert_eq!(
            document.matches(&format!("crs:GrayMixer{band}=")).count(),
            1,
            "{band}"
        );
        assert!(document.contains(&format!(r#"crs:GrayMixer{band}="0""#)));
    }
    assert!(!document.contains("crs:ShadowTint"));
}

#[test]
fn neutral_recipe_does_not_invent_calibration_or_black_and_white() {
    let document = autoshade::xmp::recipe_to_xmp(&EditRecipe::default());

    assert!(!document.contains("crs:ShadowTint"));
    assert!(!document.contains("crs:GrayMixer"));
    assert!(!document.contains("crs:ConvertToGrayscale"));
}

#[test]
fn false_black_and_white_switch_imports_as_color() {
    let document =
        lightroom_v150_globals_document().replace(r#"crs:ConvertToGrayscale="True""#, r#"crs:ConvertToGrayscale="False""#);

    assert!(!autoshade::xmp::xmp_to_recipe(&document).convert_to_grayscale);
}

const LIGHTROOM_POINT_COLOR_SWATCH: &str =
    "1.312043, 0.473663, 0.739782, -1.000000, -1.000000, -1.000000, 0.500000, \
     0.000000, 0.330000, 0.670000, 1.000000, 0.000000, 0.290000, 0.650000, 1.000000, \
     0.150000, 0.700000, 1.000000, 1.000000";

fn lightroom_point_color_placeholder() -> String {
    ["-1.000000"; 19].join(", ")
}

fn lightroom_point_colors_document(items: &[&str]) -> String {
    let items: String = items
        .iter()
        .map(|item| format!("<rdf:li>{item}</rdf:li>"))
        .collect();
    format!(
        r#"<rdf:Description xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"
 crs:Exposure2012="+0.35" crs:HasSettings="True">
 <crs:PointColors><rdf:Seq>{items}</rdf:Seq></crs:PointColors>
</rdf:Description>"#
    )
}

#[test]
fn lightroom_no_swatch_placeholder_stays_one_untouched_element() {
    let placeholder = lightroom_point_color_placeholder();
    let document = lightroom_point_colors_document(&[placeholder.as_str()]);
    let imported = autoshade::xmp::xmp_to_recipe(&document);

    assert!(imported.point_colors.is_empty());
    assert!(autoshade::xmp::unparsable_crs_numbers(&document).is_empty());
    assert!(autoshade::xmp::unmodelled_global_crs(&document).is_empty());
    let merged = autoshade::xmp::merge_recipe_into_xmp(&document, &imported).unwrap();
    assert_eq!(merged.doc.matches("<crs:PointColors>").count(), 1);
    assert!(merged.doc.contains(&placeholder));
}

#[test]
fn lightroom_point_color_merges_once_and_reopens_number_for_number() {
    let document = lightroom_point_colors_document(&[LIGHTROOM_POINT_COLOR_SWATCH]);
    let imported = autoshade::xmp::xmp_to_recipe(&document);
    assert_eq!(imported.point_colors.len(), 1);
    let point = &imported.point_colors[0];
    assert_eq!(
        (point.src_hue, point.src_sat, point.src_lum),
        (1.312043, 0.473663, 0.739782)
    );
    assert_eq!(
        (
            point.hue_shift,
            point.sat_scale,
            point.lum_scale,
            point.range_amount,
        ),
        (-1.0, -1.0, -1.0, 0.5)
    );
    assert_eq!(point.hue_range, [0.0, 0.33, 0.67, 1.0]);
    assert_eq!(point.sat_range, [0.0, 0.29, 0.65, 1.0]);
    assert_eq!(point.lum_range, [0.15, 0.7, 1.0, 1.0]);

    let merged = autoshade::xmp::merge_recipe_into_xmp(&document, &imported).unwrap();
    assert_eq!(merged.doc.matches("<crs:PointColors>").count(), 1);
    assert!(merged.doc.contains(&format!(
        "<rdf:li>{LIGHTROOM_POINT_COLOR_SWATCH}</rdf:li>"
    )));
    assert_eq!(
        autoshade::xmp::xmp_to_recipe(&merged.doc).point_colors,
        imported.point_colors
    );
}

#[test]
fn point_colors_export_between_global_curves_and_mask_corrections() {
    let point = autoshade::xmp::xmp_to_recipe(&lightroom_point_colors_document(&[
        LIGHTROOM_POINT_COLOR_SWATCH,
    ]))
    .point_colors
    .remove(0);
    let document = autoshade::xmp::recipe_to_xmp(&EditRecipe {
        tone_curve: vec![
            CurvePoint {
                input: 0,
                output: 0,
            },
            CurvePoint {
                input: 128,
                output: 140,
            },
            CurvePoint {
                input: 255,
                output: 255,
            },
        ],
        point_colors: vec![point],
        masks: vec![LocalAdjustment {
            mask: MaskGeometry::Linear {
                zero_x: 0.0,
                zero_y: 0.0,
                full_x: 0.0,
                full_y: 1.0,
            },
            enabled: true,
            amount: 1.0,
            exposure_ev: 0.5,
            ..LocalAdjustment::default()
        }],
        ..EditRecipe::default()
    });

    let curve = document.find("</crs:ToneCurvePV2012>").unwrap();
    let points = document.find("<crs:PointColors>").unwrap();
    let masks = document.find("<crs:MaskGroupBasedCorrections>").unwrap();
    assert!(curve < points && points < masks);
}

#[test]
fn current_recipe_can_clear_lightroom_point_colors() {
    let document = lightroom_point_colors_document(&[LIGHTROOM_POINT_COLOR_SWATCH]);
    let mut current = autoshade::xmp::xmp_to_recipe(&document);
    current.point_colors.clear();

    let merged = autoshade::xmp::merge_recipe_into_xmp(&document, &current).unwrap();

    assert!(!merged.doc.contains("<crs:PointColors>"));
}

#[test]
fn v1_4_recipe_preserves_point_colors_it_never_represented() {
    let document = lightroom_point_colors_document(&[LIGHTROOM_POINT_COLOR_SWATCH]);
    let legacy: EditRecipe =
        serde_json::from_str(r#"{"schema_era":1,"exposure_ev":0.35}"#).unwrap();

    let merged = autoshade::xmp::merge_recipe_into_xmp(&document, &legacy).unwrap();

    assert_eq!(merged.doc.matches("<crs:PointColors>").count(), 1);
    assert!(merged.doc.contains(&format!(
        "<rdf:li>{LIGHTROOM_POINT_COLOR_SWATCH}</rdf:li>"
    )));
}

#[test]
fn real_point_color_replaces_the_lightroom_placeholder_once() {
    let placeholder = lightroom_point_color_placeholder();
    let base = lightroom_point_colors_document(&[placeholder.as_str()]);
    let point = autoshade::xmp::xmp_to_recipe(&lightroom_point_colors_document(&[
        LIGHTROOM_POINT_COLOR_SWATCH,
    ]));

    let merged = autoshade::xmp::merge_recipe_into_xmp(&base, &point).unwrap();

    assert_eq!(merged.doc.matches("<crs:PointColors>").count(), 1);
    assert!(!merged.doc.contains(&placeholder));
    assert!(merged.doc.contains(&format!(
        "<rdf:li>{LIGHTROOM_POINT_COLOR_SWATCH}</rdf:li>"
    )));
    assert!(merged.notes.is_empty());
}

fn unreadable_point_color_documents() -> Vec<String> {
    let eighteen = LIGHTROOM_POINT_COLOR_SWATCH.rsplit_once(", ").unwrap().0;
    let twenty = format!("{LIGHTROOM_POINT_COLOR_SWATCH}, 0.000000");
    let saturation_above_one =
        LIGHTROOM_POINT_COLOR_SWATCH.replacen("0.473663", "1.473663", 1);
    let backwards_window =
        LIGHTROOM_POINT_COLOR_SWATCH.replacen("0.290000, 0.650000", "0.650000, 0.290000", 1);
    let too_many = vec![
        LIGHTROOM_POINT_COLOR_SWATCH;
        autoshade::recipe::MAX_POINT_COLORS + 1
    ];
    vec![
        lightroom_point_colors_document(&[eighteen]),
        lightroom_point_colors_document(&[twenty.as_str()]),
        lightroom_point_colors_document(&[saturation_above_one.as_str()]),
        lightroom_point_colors_document(&[backwards_window.as_str()]),
        lightroom_point_colors_document(&[LIGHTROOM_POINT_COLOR_SWATCH, "not, a, number"]),
        lightroom_point_colors_document(&too_many),
    ]
}

fn point_colors_element(document: &str) -> &str {
    let start = document.find("<crs:PointColors>").unwrap();
    let end = document.find("</crs:PointColors>").unwrap() + "</crs:PointColors>".len();
    &document[start..end]
}

#[test]
fn unreadable_point_color_records_are_refused_and_named() {
    for document in unreadable_point_color_documents() {
        assert_eq!(
            autoshade::xmp::unparsable_crs_numbers(&document),
            vec!["PointColors"]
        );
        assert!(autoshade::xmp::xmp_to_recipe(&document)
            .point_colors
            .is_empty());
    }
}

#[test]
fn unreadable_point_color_block_is_preserved_without_a_replacement() {
    for document in unreadable_point_color_documents() {
        let original = point_colors_element(&document);
        let recipe = autoshade::xmp::xmp_to_recipe(&document);

        let merged = autoshade::xmp::merge_recipe_into_xmp(&document, &recipe).unwrap();

        assert!(merged.doc.contains(original));
        assert!(merged.notes.is_empty());
    }
}

#[test]
fn unreadable_point_color_block_is_replaced_with_disclosure() {
    let replacement = autoshade::xmp::xmp_to_recipe(&lightroom_point_colors_document(&[
        LIGHTROOM_POINT_COLOR_SWATCH,
    ]));
    for document in unreadable_point_color_documents() {
        let original = point_colors_element(&document);

        let merged = autoshade::xmp::merge_recipe_into_xmp(&document, &replacement).unwrap();

        assert_eq!(merged.doc.matches("<crs:PointColors>").count(), 1);
        assert!(!merged.doc.contains(original));
        assert_eq!(merged.notes.len(), 1);
        assert!(merged.notes[0].contains("point colours could not be read"));
    }
}

#[test]
fn v1_4_recipe_only_replaces_globals_from_its_own_era() {
    let legacy: EditRecipe =
        serde_json::from_str(r#"{"schema_era":1,"exposure_ev":0.35,"texture":0.0}"#).unwrap();
    let merged =
        autoshade::xmp::merge_recipe_into_xmp(lightroom_v150_globals_document(), &legacy)
            .unwrap();

    assert!(merged.doc.contains(r#"crs:Texture="0""#));
    for spelling in [
        r#"crs:CameraProfile="Adobe Standard""#,
        r#"crs:LensProfileSetup="LensDefaults""#,
        r#"crs:LensProfileName="Adobe Synthetic Lens""#,
        r#"crs:LensProfileFilename="Synthetic.lcp""#,
        r#"crs:LensProfileDistortionScale="65""#,
        r#"crs:LensProfileVignettingScale="75""#,
        r#"crs:PerspectiveVertical="11""#,
        r#"crs:PerspectiveHorizontal="-12""#,
        r#"crs:PerspectiveRotate="+0.9""#,
        r#"crs:PerspectiveScale="90""#,
        r#"crs:PerspectiveAspect="13""#,
        r#"crs:PerspectiveX="+1.25""#,
        r#"crs:PerspectiveY="-2.50""#,
        r#"crs:PerspectiveUpright="3""#,
        r#"crs:CropConstrainToWarp="1""#,
        r#"crs:ParametricShadows="-10""#,
        r#"crs:ParametricDarks="+20""#,
        r#"crs:ParametricLights="-30""#,
        r#"crs:ParametricHighlights="+40""#,
        r#"crs:ParametricShadowSplit="20""#,
        r#"crs:ParametricMidtoneSplit="55""#,
        r#"crs:ParametricHighlightSplit="85""#,
        r#"crs:ShadowTint="+1""#,
        r#"crs:RedHue="+2""#,
        r#"crs:RedSaturation="+3""#,
        r#"crs:GreenHue="+4""#,
        r#"crs:GreenSaturation="+5""#,
        r#"crs:BlueHue="+6""#,
        r#"crs:BlueSaturation="+7""#,
        r#"crs:ConvertToGrayscale="True""#,
        r#"crs:GrayMixerRed="+11""#,
        r#"crs:GrayMixerOrange="+12""#,
        r#"crs:GrayMixerYellow="+13""#,
        r#"crs:GrayMixerGreen="+14""#,
        r#"crs:GrayMixerAqua="+15""#,
        r#"crs:GrayMixerBlue="+16""#,
        r#"crs:GrayMixerPurple="+17""#,
        r#"crs:GrayMixerMagenta="+18""#,
        r#"crs:HDREditMode="1""#,
        r#"crs:HDRMaxValue="+1.00""#,
        r#"crs:SDRBlend="+10""#,
        r#"crs:SDRBrightness="+11""#,
        r#"crs:SDRContrast="+12""#,
        r#"crs:SDRHighlights="-13""#,
        r#"crs:SDRShadows="+14""#,
        r#"crs:SDRWhites="+15""#,
        r#"crs:SDRClarity="+16""#,
    ] {
        assert!(merged.doc.contains(spelling), "{spelling}");
    }
}

#[test]
fn edited_v1_4_parametric_region_rewrites_the_whole_curve_once() {
    let legacy: EditRecipe = serde_json::from_str(
        r#"{"schema_era":1,"exposure_ev":0.35,"texture":0.0,"param_lights":30.0}"#,
    )
    .unwrap();
    let merged =
        autoshade::xmp::merge_recipe_into_xmp(lightroom_v150_globals_document(), &legacy)
            .unwrap();

    for key in [
        "ParametricShadows",
        "ParametricDarks",
        "ParametricLights",
        "ParametricHighlights",
        "ParametricShadowSplit",
        "ParametricMidtoneSplit",
        "ParametricHighlightSplit",
    ] {
        assert_eq!(merged.doc.matches(&format!("crs:{key}=")).count(), 1, "{key}");
    }
    assert!(merged.doc.contains(r#"crs:ParametricLights="+30""#));
    assert!(merged.doc.contains(r#"crs:ParametricDarks="0""#));
    assert!(merged.doc.contains(r#"crs:Texture="0""#));
}

fn table_correction(name: &str, token: &str, expected: usize) -> String {
    format!(
        r#"<rdf:li><rdf:Description crs:What="Correction" crs:CorrectionName="{name}">
<crs:CorrectionMasks><rdf:Seq>
 <rdf:li crs:What="Mask/Aggregate" crs:MaskActive="true"
  crs:MaskName="{name}" crs:MaskBlendMode="0" crs:MaskInverted="false"
  crs:MaskValue="1" crs:MaskBrushTable="{token}"
  crs:MaskBrushUncompressedBytes="{expected}"/>
</rdf:Seq></crs:CorrectionMasks>
</rdf:Description></rdf:li>"#
    )
}

fn table_document(corrections: &str) -> String {
    format!(
        r#"<rdf:Description xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/">
<crs:MaskGroupBasedCorrections><rdf:Seq>{corrections}</rdf:Seq></crs:MaskGroupBasedCorrections>
</rdf:Description>"#
    )
}

#[test]
fn mask_brush_tables_import_in_owner_and_table_order() {
    let objects = vec![
        brush_object(MASK_BRUSH_GOOD_A),
        brush_object(MASK_BRUSH_GOOD_B),
    ];
    let (acr, tokens) = many_object_acr(&objects);
    let tree = support::TempTree::new("brush-table-order");
    let photo = tree.path().join("photo.dng");
    std::fs::write(&photo, b"synthetic photo").unwrap();
    std::fs::write(photo.with_extension("acr"), acr).unwrap();
    let xmp = table_document(&format!(
        "{}{}",
        table_correction("First", &tokens[0], 185),
        table_correction("Second", &tokens[1], 87),
    ));
    let collector = autoshade::diag::Collector::new();
    let diag = autoshade::diag::Diag::about(&collector, &photo);

    let recipe = autoshade::xmp::xmp_to_recipe_with_diag(&xmp, &diag);

    assert!(collector.is_empty());
    assert_eq!(recipe.masks.len(), 2);
    let MaskGeometry::Brush { strokes: first, .. } = &recipe.masks[0].mask else {
        panic!("first table imports as a brush");
    };
    let MaskGeometry::Brush {
        strokes: second, ..
    } = &recipe.masks[1].mask
    else {
        panic!("second table imports as a brush");
    };
    assert_eq!(first.len(), 2);
    assert_eq!(second.len(), 1);
    assert_eq!(first[0].sync_id, "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA");
    assert_eq!(first[1].sync_id, "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB");
    assert_eq!(second[0].sync_id, "CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC");
}

#[test]
fn unchanged_table_mask_merge_keeps_table_attributes_without_duplicate_paints() {
    let object = brush_object(MASK_BRUSH_GOOD_A);
    let (acr, token) = one_object_acr(&object);
    let tree = support::TempTree::new("brush-table-merge");
    let photo = tree.path().join("photo.dng");
    std::fs::write(&photo, b"synthetic photo").unwrap();
    std::fs::write(photo.with_extension("acr"), acr).unwrap();
    let xmp = table_document(&table_correction("Mask 1", &token, 185));
    let mut recipe = autoshade::xmp::xmp_to_recipe_for_photo(&xmp, &photo);
    recipe.exposure_ev = 0.75;

    let merged = autoshade::xmp::merge_recipe_into_xmp_in_frame_for_photo(
        &xmp,
        &recipe,
        None,
        Some(&photo),
    )
    .unwrap();

    assert_eq!(merged.doc.matches("crs:MaskBrushTable=").count(), 1);
    assert!(merged
        .doc
        .contains(&format!("crs:MaskBrushTable=\"{token}\"")));
    assert!(merged
        .doc
        .contains(r#"crs:MaskBrushUncompressedBytes="185""#));
    assert_eq!(merged.doc.matches(r#"crs:What="Mask/Paint""#).count(), 0);
}

fn inline_paint(sync_id: &str, dabs: &[&str], extra: &str) -> String {
    let items = dabs
        .iter()
        .map(|dab| format!("<rdf:li>{dab}</rdf:li>"))
        .collect::<String>();
    format!(
        r#"<rdf:li><rdf:Description crs:What="Mask/Paint" crs:MaskActive="true"
 crs:MaskBlendMode="0" crs:MaskInverted="false" crs:MaskSyncID="{sync_id}"
 crs:MaskValue="1" crs:Radius="0.5" crs:Flow="1" crs:CenterWeight="0" {extra}>
 <crs:Dabs><rdf:Seq>{items}</rdf:Seq></crs:Dabs>
</rdf:Description></rdf:li>"#
    )
}

#[test]
fn table_import_preserves_residual_aggregate_and_ai_gesture_paints() {
    let object = brush_object(MASK_BRUSH_GOOD_B);
    let (acr, token) = one_object_acr(&object);
    let tree = support::TempTree::new("brush-table-survivors");
    let photo = tree.path().join("photo.dng");
    std::fs::write(&photo, b"synthetic photo").unwrap();
    std::fs::write(photo.with_extension("acr"), acr).unwrap();
    let aggregate_paint = inline_paint(
        "1111111111111111111111111111111A",
        &["d 0.5 0.5"],
        "",
    );
    let gesture_paint = inline_paint(
        "1111111111111111111111111111111B",
        &["h 1.0000", "d 0.25 0.75"],
        r#"crs:BrushGestureInterpretation="0""#,
    );
    let corrections = format!(
        r#"<rdf:li><rdf:Description crs:What="Correction" crs:CorrectionName="Brushes">
<crs:CorrectionMasks><rdf:Seq>
 <rdf:li crs:What="Mask/Aggregate" crs:MaskActive="true"
  crs:MaskName="Binary" crs:MaskBlendMode="0" crs:MaskInverted="false"
  crs:MaskValue="1" crs:MaskBrushTable="{token}"
  crs:MaskBrushUncompressedBytes="87"/>
 <rdf:li><rdf:Description crs:What="Mask/Aggregate" crs:MaskActive="true"
  crs:MaskName="Inline" crs:MaskBlendMode="0" crs:MaskInverted="false"
  crs:MaskValue="1"><crs:Masks><rdf:Seq>{aggregate_paint}</rdf:Seq></crs:Masks>
 </rdf:Description></rdf:li>
</rdf:Seq></crs:CorrectionMasks>
</rdf:Description></rdf:li>
<rdf:li><rdf:Description crs:What="Correction" crs:CorrectionName="Gesture">
<crs:CorrectionMasks><rdf:Seq>
 <rdf:li><rdf:Description crs:What="Mask/Image" crs:MaskActive="true"
  crs:MaskName="Subject" crs:MaskBlendMode="0" crs:MaskInverted="false"
  crs:MaskValue="1" crs:MaskVersion="1" crs:MaskSubType="0"
  crs:ReferencePoint="0.5 0.5"><crs:Gesture><rdf:Seq>{gesture_paint}</rdf:Seq></crs:Gesture>
 </rdf:Description></rdf:li>
</rdf:Seq></crs:CorrectionMasks>
</rdf:Description></rdf:li>"#
    );
    let xmp = table_document(&corrections);

    let recipe = autoshade::xmp::xmp_to_recipe_for_photo(&xmp, &photo);

    assert_eq!(recipe.masks.len(), 2);
    assert_eq!(recipe.masks[0].components.len(), 1);
    let MaskGeometry::AiMask { gesture, .. } = &recipe.masks[1].mask else {
        panic!("the second correction remains an AI mask");
    };
    assert_eq!(gesture.len(), 1);
    assert!(gesture[0].dabs.starts_with("h 1.0000\n"));
    let merged = autoshade::xmp::merge_recipe_into_xmp_in_frame_for_photo(
        &xmp,
        &recipe,
        None,
        Some(&photo),
    )
    .unwrap();
    assert!(merged
        .doc
        .contains(r#"crs:BrushGestureInterpretation="0""#));
}

#[test]
#[ignore = "requires the external controlled Lightroom radial probe sidecars"]
fn real_lightroom_radials_round_trip_declared_geometry() {
    let root = std::env::var_os("AUTOSHADE_LR_PROBE_FIXTURES")
        .map(std::path::PathBuf::from)
        .expect("set AUTOSHADE_LR_PROBE_FIXTURES to the controlled Lightroom probe directory");
    assert!(root.is_dir(), "{} is not a fixture directory", root.display());

    let mut pending = vec![root];
    let mut files = Vec::new();
    while let Some(directory) = pending.pop() {
        let entries = std::fs::read_dir(&directory)
            .unwrap_or_else(|error| panic!("{}: {error}", directory.display()));
        for entry in entries {
            let path = entry
                .unwrap_or_else(|error| panic!("{}: {error}", directory.display()))
                .path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("xmp")) {
                files.push(path);
            }
        }
    }
    files.sort();

    let mut checked = 0usize;
    for path in files {
        let input = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        if !input.contains("Mask/CircularGradient") {
            continue;
        }
        let frame = declared_public_frame(&input)
            .unwrap_or_else(|| panic!("{}: missing or invalid declared frame", path.display()));
        let recipe = autoshade::xmp::xmp_to_recipe(&input);
        assert!(!recipe.masks.is_empty(), "{}: radial did not import", path.display());
        let output = autoshade::xmp::recipe_to_xmp_in_frame(&recipe, Some(frame)).0;

        for key in ["Top", "Left", "Bottom", "Right"] {
            let name = format!("crs:{key}");
            assert_eq!(
                xmp_public_value(&output, &name),
                xmp_public_value(&input, &name),
                "{}: {name}",
                path.display()
            );
        }
        if let Some(want) = xmp_public_value(&input, "crs:Angle") {
            let got = xmp_public_value(&output, "crs:Angle")
                .unwrap_or_else(|| panic!("{}: output lost crs:Angle", path.display()));
            let (want, got): (f64, f64) = (want.parse().unwrap(), got.parse().unwrap());
            assert!((want - got).abs() < 1e-4, "{}: angle {got} vs {want}", path.display());
        }
        for key in ["HasCrop", "CropTop", "CropLeft", "CropBottom", "CropRight", "CropAngle"] {
            let name = format!("crs:{key}");
            assert_eq!(
                xmp_public_value(&output, &name),
                xmp_public_value(&input, &name),
                "{}: {name}",
                path.display()
            );
        }
        for key in ["tiff:ImageWidth", "tiff:ImageLength", "tiff:Orientation"] {
            let input_value = xmp_public_value(&input, key).and_then(|v| v.parse::<f64>().ok());
            let output_value = xmp_public_value(&output, key).and_then(|v| v.parse::<f64>().ok());
            assert_eq!(output_value, input_value, "{}: {key}", path.display());
        }
        checked += 1;
    }
    assert!(checked > 0, "fixture directory held no radial sidecar");
}

#[test]
#[ignore = "requires the external P04 rewritten MaskBrushTable XMP/ARW/ACR fixture"]
fn real_mask_brush_sample_matches_stage_three_ground_truth() {
    let root = std::env::var_os("AUTOSHADE_MB_SAMPLE_ROOT")
        .map(std::path::PathBuf::from)
        .expect("set AUTOSHADE_MB_SAMPLE_ROOT to the P04 rewritten fixture directory");
    let xmp = std::fs::read_to_string(root.join("P04-rewritten-brushtable.xmp")).unwrap();
    let photo = root.join("P04-rewritten.arw");
    let collector = autoshade::diag::Collector::new();
    let diag = autoshade::diag::Diag::about(&collector, &photo);

    let recipe = autoshade::xmp::xmp_to_recipe_with_diag(&xmp, &diag);
    assert!(collector.is_empty());
    let tables = recipe
        .masks
        .iter()
        .flat_map(|mask| {
            std::iter::once(&mask.mask)
                .chain(mask.components.iter().map(|component| &component.geometry))
        })
        .filter_map(|geometry| match geometry {
            MaskGeometry::Brush { strokes, .. } if matches!(strokes.len(), 54 | 4 | 18) => {
                Some(strokes)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        tables.iter().map(|table| table.len()).collect::<Vec<_>>(),
        [54, 4, 18]
    );
    let d_count = tables
        .iter()
        .flat_map(|table| table.iter())
        .map(|stroke| {
            stroke
                .dabs
                .lines()
                .filter(|token| token.starts_with("d "))
                .count()
        })
        .sum::<usize>();
    assert_eq!(d_count, 3_043);
    assert_eq!(
        (tables[1][0].value * 1_000_000.0).round() as u32,
        51_402
    );
    assert_eq!(
        tables[1][0].dabs.lines().next(),
        Some("d 0.404621 0.692602")
    );
}
