//! The model-catalogue landing, headless: how a fetched listing is classified
//! and what the landing says. Fetching, single-flight flags and the generation
//! guard are the runtime's.
use autoshade::text::generated::*;


use super::UiText;
use std::collections::HashMap;

/// One installed catalogue, classified for the role that asked.
pub(crate) struct Catalogue {
    pub(crate) chat: Vec<String>,
    /// Only a role that generates images keeps an image list; an analysis
    /// catalogue holding ids from an image endpoint would offer the wrong
    /// picker.
    pub(crate) image: Vec<String>,
    /// What the listing SAID about each id, kept only where it said it.
    pub(crate) efforts: HashMap<String, Vec<String>>,
    pub(crate) vision: HashMap<String, bool>,
    pub(crate) total: usize,
}

/// Classify one listing for a role.
pub(crate) fn catalogue(
    models: &[autoshade::openai_models::ModelInfo],
    role_generates_images: bool,
) -> Catalogue {
    let chat: Vec<String> =
        models.iter().filter(|m| autoshade::openai_models::is_chat_model(&m.id)).map(|m| m.id.clone()).collect();
    let image: Vec<String> = if role_generates_images {
        models.iter().filter(|m| autoshade::openai_models::is_image_model(&m.id)).map(|m| m.id.clone()).collect()
    } else {
        Vec::new()
    };
    let mut efforts = HashMap::new();
    let mut vision = HashMap::new();
    for m in models {
        if let Some(tiers) = &m.effort_options {
            efforts.insert(m.id.clone(), tiers.clone());
        }
        if let Some(v) = m.vision {
            vision.insert(m.id.clone(), v);
        }
    }
    Catalogue { chat, image, efforts, vision, total: models.len() }
}

/// The line for a landing that installed a listing, was discarded as stale,
/// or failed.
pub(crate) fn installed_words(catalogue: &Catalogue) -> UiText {
    UiText::with(
        K_LOGIC_FETCHED_MODELS_CHAT_IMAGE,
        &[
            ("n", &catalogue.total.to_string()),
            ("chat", &catalogue.chat.len().to_string()),
            ("img", &catalogue.image.len().to_string()),
        ],
    )
}

pub(crate) fn discarded_words() -> UiText {
    UiText::of(K_LOGIC_MODEL_LIST_DISCARDED_SETTINGS_CHANGED_WHILE_IT_W)
}

pub(crate) fn failed_words(err: &str) -> UiText {
    UiText::with(K_LOGIC_FETCH_FAILED, &[("err", err)])
}
