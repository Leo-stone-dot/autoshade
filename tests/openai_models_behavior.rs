use std::time::Duration;

use autoshade::openai_models::{ModelInfo, is_chat_model, is_image_model, list_models};
use serde_json::{Value, json};

#[allow(clippy::disallowed_methods)] // local HTTP stub serves while synchronous list_models blocks
fn list_from(value: Value) -> anyhow::Result<Vec<ModelInfo>> {
    let body = serde_json::to_string(&value).unwrap();
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let base = format!("http://{}", server.server_addr());
    let worker = std::thread::spawn(move || {
        let request = server
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .expect("list_models must issue one request");
        let content_type =
            tiny_http::Header::from_bytes("Content-Type", "application/json").unwrap();
        request
            .respond(tiny_http::Response::from_string(body).with_header(content_type))
            .unwrap();
    });

    let result = list_models(&base, "synthetic-key");
    worker.join().unwrap();
    result
}

#[test]
fn known_image_generation_ids_are_recognized_and_not_offered_as_chat() {
    for id in ["gpt-image-2", "gpt-image-1.5", "chatgpt-image-latest"] {
        assert!(is_image_model(id), "id={id}");
        assert!(!is_chat_model(id), "id={id}");
    }
}

#[test]
fn known_openai_chat_ids_are_offered_as_chat() {
    for id in ["gpt-5.5", "gpt-4o", "o3", "o4-mini"] {
        assert!(is_chat_model(id), "id={id}");
    }
}

#[test]
fn third_party_chat_ids_are_offered_as_chat() {
    for id in [
        "claude-opus-4-6",
        "qwen2.5-vl-72b-instruct",
        "deepseek-chat",
        "llama-3.3-70b-instruct",
        "mistral-large-latest",
    ] {
        assert!(is_chat_model(id), "id={id}");
    }
}

#[test]
fn non_chat_modalities_are_not_offered_as_chat() {
    for id in [
        "gpt-4o-audio-preview",
        "text-embedding-3-large",
        "Qwen/Qwen2-Audio-7B-Instruct",
        "openai/whisper-large-v3",
        "BAAI/bge-reranker-v2-m3",
        "Meta-Llama-Guard-2-8B",
        "qwen-audio-turbo",
        "bge-reranker-v2",
        "dall-e-3",
        "sora-2",
        "llama-guard-3",
    ] {
        assert!(!is_chat_model(id), "id={id}");
    }
}

#[test]
fn completion_only_models_are_not_offered_as_chat() {
    for id in ["babbage-002", "davinci-002"] {
        assert!(!is_chat_model(id), "id={id}");
    }
}

#[test]
fn an_empty_model_id_is_not_offered_as_chat() {
    assert!(!is_chat_model(""));
}

#[test]
fn a_non_image_chat_id_is_not_classified_as_an_image_model() {
    assert!(!is_image_model("gpt-5.5"));
}

#[test]
fn a_model_listing_over_the_item_limit_is_refused() {
    let data = (0..=4096)
        .map(|index| json!({"id": format!("model-{index}")}))
        .collect::<Vec<_>>();

    let error = list_from(json!({"data": data})).unwrap_err().to_string();

    assert!(error.contains("maximum"), "error={error}");
}

#[test]
fn an_oversized_model_id_is_refused() {
    let error = list_from(json!({
        "data": [{"id": "x".repeat(257)}]
    }))
    .unwrap_err()
    .to_string();

    assert!(error.contains("identifier longer"), "error={error}");
}

#[test]
fn a_model_id_with_control_characters_is_refused() {
    let error = list_from(json!({
        "data": [{"id": "safe\nforged"}]
    }))
    .unwrap_err()
    .to_string();

    assert!(error.contains("control characters"), "error={error}");
}

#[test]
fn a_plain_model_listing_keeps_capabilities_unknown() {
    assert_eq!(
        list_from(json!({"data": [{"id": "plain"}]})).unwrap(),
        [ModelInfo::from("plain")]
    );
}

#[test]
fn a_venice_reasoner_listing_exposes_sanitized_tiers_and_vision() {
    let models = list_from(json!({"data": [{
        "id": "venice-reasoner",
        "model_spec": {"capabilities": {
            "supportsReasoning": true,
            "supportsReasoningEffort": true,
            "reasoningEffortOptions": ["low", "medium", "high", "Bad Tier!"],
            "supportsVision": true
        }}
    }]}))
    .unwrap();

    assert_eq!(
        models,
        [ModelInfo {
            id: "venice-reasoner".to_string(),
            effort_options: Some(["low", "medium", "high"].map(String::from).to_vec()),
            vision: Some(true),
        }]
    );
}

#[test]
fn a_venice_non_reasoner_listing_exposes_no_tiers_and_no_vision() {
    let models = list_from(json!({"data": [{
        "id": "venice-plain",
        "model_spec": {"capabilities": {
            "supportsReasoning": false,
            "supportsVision": false
        }}
    }]}))
    .unwrap();

    assert_eq!(
        models,
        [ModelInfo {
            id: "venice-plain".to_string(),
            effort_options: Some(Vec::new()),
            vision: Some(false),
        }]
    );
}

#[test]
fn an_openrouter_vision_listing_exposes_reasoning_tiers_and_vision() {
    let models = list_from(json!({"data": [{
        "id": "router-vision",
        "architecture": {"input_modalities": ["text", "image"]},
        "supported_parameters": ["temperature", "reasoning"]
    }]}))
    .unwrap();

    assert_eq!(
        models,
        [ModelInfo {
            id: "router-vision".to_string(),
            effort_options: Some(["low", "medium", "high"].map(String::from).to_vec()),
            vision: Some(true),
        }]
    );
}

#[test]
fn an_openrouter_text_listing_exposes_no_tiers_and_no_vision() {
    let models = list_from(json!({"data": [{
        "id": "router-text",
        "architecture": {"modality": "text->text"},
        "supported_parameters": ["temperature"]
    }]}))
    .unwrap();

    assert_eq!(
        models,
        [ModelInfo {
            id: "router-text".to_string(),
            effort_options: Some(Vec::new()),
            vision: Some(false),
        }]
    );
}
