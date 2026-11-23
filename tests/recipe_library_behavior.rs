mod support;

use autoshade::recipe::{EditRecipe, LocalAdjustment, MaskGeometry, MaskRole};
use autoshade::recipe_library::{self, NameError};
use support::{ScopedEnv, TempTree};

fn recipe() -> EditRecipe {
    EditRecipe {
        exposure_ev: 0.42,
        contrast: 12.0,
        ..EditRecipe::default()
    }
}

#[test]
fn creating_a_recipe_library_entry_writes_a_plain_recipe_file() {
    let tree = TempTree::new("recipe-create");
    recipe_library::create_in(tree.path(), "Late sun", &recipe()).unwrap();

    let path = recipe_library::entry_path_in(tree.path(), "Late sun");
    let parsed: EditRecipe = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();

    assert_eq!(parsed, recipe());
}

#[test]
fn listing_and_reading_a_recipe_library_entry_returns_its_name_and_look() {
    let tree = TempTree::new("recipe-list");
    recipe_library::create_in(tree.path(), "Late sun", &recipe()).unwrap();

    let listed = recipe_library::list_in(tree.path());

    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].name, "Late sun");
    assert!(listed[0].error.is_none());
    assert_eq!(
        recipe_library::read_in(tree.path(), "Late sun").unwrap(),
        recipe()
    );
}

#[test]
fn renaming_to_a_taken_recipe_name_preserves_both_entries() {
    let tree = TempTree::new("recipe-rename-collision");
    let first = EditRecipe {
        exposure_ev: 0.5,
        ..EditRecipe::default()
    };
    let second = EditRecipe {
        contrast: 20.0,
        ..EditRecipe::default()
    };
    recipe_library::create_in(tree.path(), "first", &first).unwrap();
    recipe_library::create_in(tree.path(), "second", &second).unwrap();

    assert!(recipe_library::rename_in(tree.path(), "second", "first").is_err());
    assert_eq!(
        recipe_library::read_in(tree.path(), "first").unwrap(),
        first
    );
    assert_eq!(
        recipe_library::read_in(tree.path(), "second").unwrap(),
        second
    );
}

#[test]
fn a_malformed_recipe_entry_does_not_hide_a_valid_entry() {
    let tree = TempTree::new("recipe-malformed-and-valid");
    recipe_library::create_in(tree.path(), "Good", &recipe()).unwrap();
    let dir = recipe_library::library_dir_in(tree.path());
    std::fs::write(dir.join("Broken.json"), b"{ not json").unwrap();

    let listed = recipe_library::list_in(tree.path());
    let good = listed.iter().find(|entry| entry.name == "Good").unwrap();
    let broken = listed.iter().find(|entry| entry.name == "Broken").unwrap();

    assert_eq!(listed.len(), 2);
    assert!(good.error.is_none());
    assert!(broken.error.is_some());
}

#[test]
fn a_missing_recipe_library_lists_as_empty() {
    let tree = TempTree::new("recipe-missing-list");

    assert!(recipe_library::list_in(tree.path()).is_empty());
}

#[test]
fn a_missing_recipe_name_is_reported_as_absent() {
    let tree = TempTree::new("recipe-missing-name");
    let mut env = ScopedEnv::new();
    env.set("AUTOSHADE_DATA_DIR", tree.path());

    assert!(!recipe_library::exists("no such recipe"));
}

#[test]
fn surrounding_whitespace_is_removed_from_a_recipe_name() {
    assert_eq!(recipe_library::check_name("  Warm  ").unwrap(), "Warm");
}

#[test]
fn filesystem_reserved_recipe_name_endings_are_refused() {
    assert_eq!(
        recipe_library::check_name("a:b"),
        Err(NameError::Illegal(':'))
    );
    assert_eq!(
        recipe_library::check_name("dot."),
        Err(NameError::Illegal('.'))
    );
}

#[test]
fn an_empty_recipe_name_is_refused() {
    assert_eq!(recipe_library::check_name("   "), Err(NameError::Empty));
}

#[test]
fn an_oversized_recipe_name_is_refused() {
    assert_eq!(
        recipe_library::check_name(&"x".repeat(autoshade::store::MAX_STORE_NAME + 1)),
        Err(NameError::TooLong)
    );
}

#[test]
fn portable_recipe_drops_advanced_photo_specific_state_and_counts_each_loss() {
    let recipe = EditRecipe {
        colour_field: Some(serde_json::from_str("{}").unwrap()),
        pixel_layers: vec![serde_json::from_str("{}").unwrap()],
        lens_blur: Some(serde_json::from_str("{}").unwrap()),
        ..EditRecipe::default()
    };

    let (portable, losses) = recipe_library::portable(&recipe, true);

    assert!(portable.colour_field.is_none());
    assert!(portable.pixel_layers.is_empty());
    assert!(portable.lens_blur.is_none());
    assert!(losses.colour_field);
    assert_eq!(losses.layers, 1);
    assert!(losses.lens_blur);
}

#[test]
fn portable_recipe_keeps_a_parametric_mask_and_the_global_look() {
    let recipe = EditRecipe {
        exposure_ev: 0.42,
        masks: vec![LocalAdjustment {
            mask: MaskGeometry::Linear {
                zero_x: 0.0,
                zero_y: 0.0,
                full_x: 1.0,
                full_y: 1.0,
            },
            ..LocalAdjustment::default()
        }],
        ..EditRecipe::default()
    };

    let (portable, losses) = recipe_library::portable(&recipe, false);

    assert_eq!(portable.exposure_ev, 0.42);
    assert_eq!(portable.masks.len(), 1);
    assert_eq!(losses.masks, 0);
}

#[test]
fn a_plain_portable_recipe_loses_nothing() {
    let recipe = recipe();

    let (portable, losses) = recipe_library::portable(&recipe, true);

    assert_eq!(portable, recipe);
    assert!(losses.is_empty());
}

#[test]
fn an_intent_only_zone_mask_travels_to_another_photo() {
    let zone = LocalAdjustment {
        role: MaskRole::ZoneSky,
        mask: MaskGeometry::AiMask {
            name: "Sky 1".to_string(),
            subtype: 2,
            ref_x: 0.5,
            ref_y: 0.5,
            blend_mode: 0,
            value: 1.0,
            inverted: false,
            mask_version: 1,
            provenance: Vec::new(),
            gesture: Vec::new(),
            raster: None,
        },
        ..LocalAdjustment::default()
    };
    let recipe = EditRecipe {
        masks: vec![zone],
        ..EditRecipe::default()
    };

    let (portable, losses) = recipe_library::portable(&recipe, false);

    assert_eq!(portable.masks.len(), 1);
    assert_eq!(losses.masks, 0);
}

#[test]
fn a_zone_mask_with_a_photo_raster_does_not_travel() {
    let zone = LocalAdjustment {
        role: MaskRole::ZoneSky,
        mask: MaskGeometry::Bitmap {
            path: "zone.png".to_string(),
        },
        ..LocalAdjustment::default()
    };
    let recipe = EditRecipe {
        masks: vec![zone],
        ..EditRecipe::default()
    };

    let (portable, losses) = recipe_library::portable(&recipe, false);

    assert!(portable.masks.is_empty());
    assert_eq!(losses.masks, 1);
}

#[test]
fn importing_an_xmp_recipe_uses_the_public_xmp_reader() {
    let tree = TempTree::new("recipe-xmp");
    let path = tree.path().join("look.xmp");
    std::fs::write(
        &path,
        r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/" crs:Exposure2012="0.35"/></rdf:RDF></x:xmpmeta>"#,
    )
    .unwrap();

    let imported = recipe_library::read_file(&path).unwrap();

    assert!((imported.exposure_ev - 0.35).abs() < 1e-6);
}
