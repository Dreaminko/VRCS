use std::collections::HashSet;

use super::*;
use crate::providers::{self, CAPABILITY_SPEECH_TO_TEXT, SERVICE_FUN_ASR_REALTIME};

impl VadConfig {
    pub fn validate(&self) -> Result<(), String> {
        if !["silence", "smart_turn"].contains(&self.endpointing.as_str()) {
            return Err(format!(
                "Unsupported VAD endpointing mode: {}",
                self.endpointing
            ));
        }
        if !(0.1..=2.0).contains(&self.silence_seconds) {
            return Err("VAD silence_seconds must be between 0.1 and 2.0".into());
        }
        if !(1.0..=30.0).contains(&self.max_speech_seconds) {
            return Err("VAD max_speech_seconds must be between 1.0 and 30.0".into());
        }
        Ok(())
    }
}

pub const ASR_LANGUAGES: [&str; 14] = [
    "zh", "en", "ja", "ko", "yue", "es", "fr", "de", "ru", "pt", "it", "th", "vi", "id",
];
pub const MAX_ASR_PREFERRED_LANGUAGES: usize = 4;

fn validate_asr_language_list(languages: &[String], label: &str) -> Result<(), String> {
    if languages.len() > MAX_ASR_PREFERRED_LANGUAGES {
        return Err(format!(
            "{label} cannot exceed {MAX_ASR_PREFERRED_LANGUAGES} languages"
        ));
    }
    let mut seen = HashSet::new();
    for language in languages {
        if !ASR_LANGUAGES.contains(&language.as_str()) {
            return Err(format!("Unsupported {label}: {language}"));
        }
        if !seen.insert(language.as_str()) {
            return Err(format!("Duplicate {label}: {language}"));
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsValidationError {
    pub code: &'static str,
    pub detail: String,
    pub params: Option<serde_json::Value>,
}

impl SettingsValidationError {
    fn invalid(detail: impl Into<String>) -> Self {
        Self {
            code: "settings.invalid",
            detail: detail.into(),
            params: None,
        }
    }

    fn coded(code: &'static str, detail: impl Into<String>) -> Self {
        Self {
            code,
            detail: detail.into(),
            params: None,
        }
    }

    fn with_params(
        code: &'static str,
        params: serde_json::Value,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            code,
            detail: detail.into(),
            params: Some(params),
        }
    }
}

impl From<String> for SettingsValidationError {
    fn from(detail: String) -> Self {
        Self::invalid(detail)
    }
}

impl From<&str> for SettingsValidationError {
    fn from(detail: &str) -> Self {
        Self::invalid(detail)
    }
}

impl AppConfig {
    /// Full structural validation shared by config file loading and `PUT /api/settings`.
    pub fn validate_settings(&self) -> Result<(), String> {
        self.validate_settings_detailed()
            .map_err(|error| error.detail)
    }

    pub fn validate_settings_detailed(&self) -> Result<(), SettingsValidationError> {
        validate_runtime(&self.server, &self.storage, &self.external_api, &self.vrcx)?;
        validate_audio(&self.audio, &self.vad)?;
        validate_recognition_options(&self.asr)?;
        validate_api_profiles(&self.asr)?;
        for (name, backend, profile_id, model) in [
            (
                "model_defaults.text",
                self.model_defaults.text.backend,
                &self.model_defaults.text.profile_id,
                &self.model_defaults.text.model,
            ),
            (
                "model_defaults.translation",
                self.model_defaults.translation.backend,
                &self.model_defaults.translation.profile_id,
                &self.model_defaults.translation.model,
            ),
            (
                "model_defaults.vision",
                self.model_defaults.vision.backend,
                &self.model_defaults.vision.profile_id,
                &self.model_defaults.vision.model,
            ),
            (
                "translation.model_selection",
                self.translation.model_selection.backend,
                &self.translation.model_selection.profile_id,
                &self.translation.model_selection.model,
            ),
            (
                "ocr.model_selection",
                self.ocr.model_selection.backend,
                &self.ocr.model_selection.profile_id,
                &self.ocr.model_selection.model,
            ),
            (
                "learning_ai",
                TranslationBackend::Api,
                &self.learning_ai.profile_id,
                &self.learning_ai.model,
            ),
        ] {
            if name.starts_with("model_defaults.")
                && name != "model_defaults.translation"
                && backend != TranslationBackend::Api
            {
                return Err(format!("{name} does not support the local PLaMo backend").into());
            }
            if profile_id
                .as_ref()
                .is_some_and(|id| id.is_empty() || id.len() > 64 || contains_control(id))
            {
                return Err(format!("Invalid {name}.profile_id").into());
            }
            if model.len() > 200 || contains_control(model) || model.trim() != model {
                return Err(format!(
                    "{name}.model must be a single-line identifier of at most 200 bytes"
                )
                .into());
            }
        }
        if self.model_defaults.translation.backend == TranslationBackend::PlamoLocal {
            if self.model_defaults.translation.profile_id.is_some() {
                return Err(
                    "model_defaults.translation cannot select an API profile for local PLaMo translation"
                        .into(),
                );
            }
            if self.model_defaults.translation.model != crate::translation::MODEL_ID {
                return Err(format!(
                    "model_defaults.translation.model must be {}",
                    crate::translation::MODEL_ID
                )
                .into());
            }
        }
        if let Some(control) = &self.learning_ai.reasoning_control {
            control.validate()?;
        }
        if self.local_translation.device != "cpu" {
            return Err("Local translation device must be cpu".into());
        }
        validate_translation_selection(
            "translation.model_selection",
            &self.translation.model_selection,
        )?;
        validate_translation_selection("ocr.model_selection", &self.ocr.model_selection)?;
        validate_glossary(&self.glossary)?;
        validate_translation(&self.translation, &self.asr.api_profiles, &self.asr.backend)?;
        validate_language_presets(
            &self.language_presets,
            &self.asr.api_profiles,
            &self.asr.backend,
            self.translation.model_selection.use_default_model,
        )?;
        validate_osc(&self.osc, &self.unitext)?;
        validate_unitext(&self.unitext)?;
        validate_recognition_models(&self.asr, &self.vad)?;
        validate_anki(&self.anki)?;
        validate_vr_overlay(&self.vr_overlay)?;
        validate_vision(self)?;
        validate_conversation_titles(self)?;
        if self.vrchat_player_id.chars().count() > 128 || contains_control(&self.vrchat_player_id) {
            return Err(
                "vrchat_player_id must not exceed 128 characters or contain control characters"
                    .into(),
            );
        }
        let ocr = &self.ocr;
        if !(5..=120).contains(&ocr.timeout_seconds)
            || !ocr.minimum_confidence.is_finite()
            || !(0.0..=1.0).contains(&ocr.minimum_confidence)
            || !ocr.region_fraction.is_finite()
            || !(0.1..=1.0).contains(&ocr.region_fraction)
            || !ocr.display_seconds.is_finite()
            || !(1.0..=120.0).contains(&ocr.display_seconds)
            || !ocr.background_opacity.is_finite()
            || !(0.0..=1.0).contains(&ocr.background_opacity)
        {
            return Err("OCR settings are outside the supported bounds".into());
        }
        if ocr.shortcut.trim().is_empty() || ocr.shortcut.len() > 64 {
            return Err("OCR shortcut must contain 1 to 64 characters".into());
        }
        validate_translation_targets(
            "ocr",
            &ocr.targets,
            &self.asr.api_profiles,
            false,
            self.ocr.model_selection.use_default_model,
            "",
        )?;
        if let Some(wrist) = &ocr.wrist {
            if !["left", "right", "dominant"].contains(&wrist.hand.as_str()) {
                return Err(format!("Unsupported OCR wrist hand: {}", wrist.hand).into());
            }
            if !["left", "right"].contains(&wrist.dominant_hand.as_str()) {
                return Err(format!(
                    "Unsupported OCR wrist dominant_hand: {}",
                    wrist.dominant_hand
                )
                .into());
            }
            for (field, value) in [
                ("offset_x_m", wrist.offset_x_m),
                ("offset_y_m", wrist.offset_y_m),
                ("offset_z_m", wrist.offset_z_m),
            ] {
                validate_range(&format!("OCR wrist {field}"), value, -0.5, 0.5)?;
            }
            for (field, value) in [
                ("pitch_deg", wrist.pitch_deg),
                ("yaw_deg", wrist.yaw_deg),
                ("roll_deg", wrist.roll_deg),
            ] {
                validate_range(&format!("OCR wrist {field}"), value, -180.0, 180.0)?;
            }
            validate_range("OCR wrist width_m", wrist.width_m, 0.1, 1.0)?;
            validate_range("OCR wrist opacity", wrist.opacity, 0.1, 1.0)?;
            if !(18..=72).contains(&wrist.font_size_px) {
                return Err("OCR wrist font_size_px must be between 18 and 72".into());
            }
        }
        Ok(())
    }
}

fn validate_conversation_titles(config: &AppConfig) -> Result<(), String> {
    let titles = &config.conversation_titles;
    if titles.model.len() > 256 || contains_control(&titles.model) {
        return Err(
            "conversation_titles.model must not exceed 256 bytes or contain control characters"
                .into(),
        );
    }
    if titles.output_language != "auto"
        && !TITLE_OUTPUT_LANGUAGES
            .iter()
            .any(|(code, _)| *code == titles.output_language)
    {
        return Err("Unsupported conversation_titles.output_language".into());
    }
    if let Some(id) = &titles.profile_id
        && (id.is_empty() || id.len() > 64 || contains_control(id))
    {
        return Err("Invalid conversation_titles.profile_id".into());
    }
    if titles.auto_generate {
        let profile = config
            .asr
            .api_profiles
            .iter()
            .find(|profile| Some(&profile.id) == titles.profile_id.as_ref())
            .ok_or("Select a text generation profile for conversation titles")?;
        providers::resolve_profile_capability(profile, providers::CAPABILITY_TEXT_GENERATION)?;
        if titles.model.trim().is_empty() {
            return Err("Select a model for conversation titles".into());
        }
    }
    Ok(())
}

fn validate_vision(config: &AppConfig) -> Result<(), String> {
    let vision = &config.vision;
    if let Some(control) = &vision.reasoning_control {
        control.validate()?;
    }
    if vision.desktop_shortcut.trim().is_empty() || vision.desktop_shortcut.len() > 64 {
        return Err("Vision desktop shortcut must contain 1 to 64 characters".into());
    }
    for (field, value, min, max) in [
        (
            "max_capture_seconds",
            vision.max_capture_seconds as u64,
            1,
            120,
        ),
        (
            "asr_finalize_timeout_ms",
            vision.asr_finalize_timeout_ms,
            100,
            120_000,
        ),
        (
            "request_timeout_seconds",
            vision.request_timeout_seconds as u64,
            1,
            600,
        ),
        (
            "max_output_tokens",
            vision.max_output_tokens as u64,
            1,
            32_768,
        ),
        ("image_max_edge", vision.image_max_edge as u64, 64, 8192),
        (
            "image_max_bytes",
            vision.image_max_bytes as u64,
            1024,
            20_971_520,
        ),
    ] {
        if !(min..=max).contains(&value) {
            return Err(format!("vision.{field} must be between {min} and {max}"));
        }
    }
    validate_range("vision.display_seconds", vision.display_seconds, 1.0, 120.0)?;
    if vision.answer_language.trim().is_empty()
        || vision.answer_language.len() > 64
        || contains_control(&vision.answer_language)
    {
        return Err(
            "vision.answer_language must contain 1 to 64 characters without control characters"
                .into(),
        );
    }
    if vision.system_prompt.trim().is_empty() || vision.system_prompt.chars().count() > 20_000 {
        return Err("vision.system_prompt must contain 1 to 20000 characters".into());
    }
    if vision.model.len() > 256 || contains_control(&vision.model) {
        return Err(
            "vision.model must not exceed 256 characters or contain control characters".into(),
        );
    }
    if let Some(id) = vision
        .profile_id
        .as_ref()
        .filter(|_| vision.enabled || !vision.use_default_model)
    {
        let profile = config
            .asr
            .api_profiles
            .iter()
            .find(|profile| &profile.id == id)
            .ok_or_else(|| format!("Unknown vision.profile_id: {id}"))?;
        crate::providers::resolve_profile_capability(
            profile,
            crate::providers::CAPABILITY_IMAGE_UNDERSTANDING,
        )
        .map_err(|detail| format!("vision.profile_id requires image_understanding: {detail}"))?;
    }
    if let Some(wrist) = &vision.wrist {
        if !["left", "right", "dominant"].contains(&wrist.hand.as_str())
            || !["left", "right"].contains(&wrist.dominant_hand.as_str())
        {
            return Err("Unsupported vision wrist hand".into());
        }
        for (field, value) in [
            ("offset_x_m", wrist.offset_x_m),
            ("offset_y_m", wrist.offset_y_m),
            ("offset_z_m", wrist.offset_z_m),
        ] {
            validate_range(&format!("vision.wrist.{field}"), value, -0.5, 0.5)?;
        }
        for (field, value) in [
            ("pitch_deg", wrist.pitch_deg),
            ("yaw_deg", wrist.yaw_deg),
            ("roll_deg", wrist.roll_deg),
        ] {
            validate_range(&format!("vision.wrist.{field}"), value, -180.0, 180.0)?;
        }
        validate_range("vision.wrist.width_m", wrist.width_m, 0.1, 1.0)?;
        validate_range("vision.wrist.opacity", wrist.opacity, 0.1, 1.0)?;
        if !(18..=72).contains(&wrist.font_size_px) {
            return Err("vision.wrist.font_size_px must be between 18 and 72".into());
        }
    }
    Ok(())
}

fn validate_runtime(
    server: &ServerConfig,
    storage: &StorageConfig,
    external_api: &ExternalApiConfig,
    vrcx: &VrcxConfig,
) -> Result<(), String> {
    if server.port == 0 {
        return Err("Port must be between 1 and 65535".into());
    }
    let external_host = external_api
        .host
        .parse::<std::net::IpAddr>()
        .map_err(|_| "External API host must be an IP address".to_string())?;
    if external_api.port == 0 {
        return Err("External API port must be between 1 and 65535".into());
    }
    if !external_host.is_loopback() && !external_api.require_token {
        return Err("External API token authentication is required outside loopback".into());
    }
    if vrcx.port == 0 {
        return Err("VRCX-0 Integration API port must be between 1 and 65535".into());
    }
    const MIN_HISTORY_BYTES: u64 = 10 * 1024 * 1024;
    const MAX_HISTORY_BYTES: u64 = 10 * 1024 * 1024 * 1024;
    if !(MIN_HISTORY_BYTES..=MAX_HISTORY_BYTES).contains(&storage.subtitle_history_max_bytes) {
        return Err("subtitle_history_max_bytes must be between 10 MiB and 10 GiB".into());
    }
    if storage.model_directory.trim().is_empty() {
        return Err("Model storage path cannot be empty".into());
    }
    Ok(())
}

fn validate_audio(audio: &AudioConfig, vad: &VadConfig) -> Result<(), String> {
    if !(8_000..=96_000).contains(&audio.sample_rate) {
        return Err("Sample rate must be between 8000 and 96000".into());
    }
    match audio.output.mode.as_str() {
        "system" => {}
        "vrchat" | "vrchat_players" | "disabled"
            if audio.output.device_id.is_some() || audio.output.device_key.is_some() =>
        {
            return Err(
                "VRChat or disabled output mode cannot specify a system output device".into(),
            );
        }
        "vrchat" | "vrchat_players" | "disabled" => {}
        other => return Err(format!("Unsupported output mode: {other}")),
    }
    let players = &audio.output.player_audio;
    if players.max_visible_players > 64 {
        return Err("Visible player limit must be between 0 and 64".into());
    }
    #[cfg(windows)]
    {
        let drive = players.pagefile_drive.as_bytes();
        if drive.len() != 2 || !drive[0].is_ascii_alphabetic() || drive[1] != b':' {
            return Err("Player audio pagefile drive must be a drive letter such as C:".into());
        }
    }
    if players.max_players > 8 {
        return Err("Player audio max_players must be 0 (unlimited) or between 1 and 8".into());
    }
    if !players.proximity.radius_m.is_finite()
        || !(0.0..=10.0).contains(&players.proximity.radius_m)
    {
        return Err("Player audio proximity.radius_m must be between 0 and 10".into());
    }
    if !players.view_priority.horizontal_fov_deg.is_finite()
        || !(30.0..=180.0).contains(&players.view_priority.horizontal_fov_deg)
    {
        return Err(
            "Player audio view_priority.horizontal_fov_deg must be between 30 and 180".into(),
        );
    }
    if let Some(ids) = &players.selected_tracks {
        let mut seen = HashSet::new();
        if ids.len() > 512
            || ids.iter().any(|id| {
                id.is_empty()
                    || id.len() > 256
                    || id.chars().any(char::is_control)
                    || !seen.insert(id)
            })
        {
            return Err("Player audio selection must contain at most 512 unique user IDs".into());
        }
    }
    if players.selected_user_names.len() > 512
        || players.selected_user_names.iter().any(|(id, name)| {
            !crate::models::is_vrchat_user_id(id)
                || name.is_empty()
                || name.chars().count() > 128
                || name.chars().any(char::is_control)
        })
    {
        return Err(
            "Saved player labels must contain valid user IDs and names of at most 128 characters"
                .into(),
        );
    }
    if players.source_overrides.len() > 512
        || players
            .source_overrides
            .keys()
            .any(|id| !crate::models::is_vrchat_user_id(id))
    {
        return Err(
            "Player source overrides must use valid user IDs for at most 512 players".into(),
        );
    }
    if !matches!(players.source_mode.as_str(), "everyone" | "allowlist") {
        return Err("Player audio source_mode must be everyone or allowlist".into());
    }
    if players.opus_library_path.len() > 4096 {
        return Err("Player audio Opus library path is too long".into());
    }
    if !(-80.0..=-10.0).contains(&audio.output.trigger_threshold_dbfs) {
        return Err("Output trigger_threshold_dbfs must be between -80 and -10".into());
    }
    match audio.microphone.mode.as_str() {
        "device"
            if audio.microphone.device_id.is_none() && audio.microphone.device_key.is_none() =>
        {
            return Err("A device must be selected in microphone device mode".into());
        }
        "device" => {}
        "default" | "disabled"
            if audio.microphone.device_id.is_some() || audio.microphone.device_key.is_some() =>
        {
            return Err("Default or disabled microphone mode cannot specify a device".into());
        }
        "default" | "disabled" => {}
        other => return Err(format!("Unsupported microphone mode: {other}")),
    }
    if !(-80.0..=-10.0).contains(&audio.microphone.trigger_threshold_dbfs) {
        return Err("Microphone trigger_threshold_dbfs must be between -80 and -10".into());
    }
    vad.validate()
}

fn validate_recognition_options(asr: &AsrConfig) -> Result<(), String> {
    if asr.backend != QWEN_MANAGED_BACKEND && providers::recognition_service(&asr.backend).is_none()
    {
        return Err(format!("Unsupported recognition backend: {}", asr.backend));
    }
    validate_asr_language_list(&asr.speaker_languages, "speaker recognition language")?;
    validate_asr_language_list(&asr.microphone_languages, "microphone recognition language")?;
    if let Some((_, service)) = providers::recognition_service(&asr.backend) {
        providers::validate_recognition_languages(service, &asr.speaker_languages)?;
        providers::validate_recognition_languages(service, &asr.microphone_languages)?;
    }
    if !asr.managed_qwen.package_id.is_empty()
        && !crate::asr::is_supported_qwen_package(&asr.managed_qwen.package_id)
    {
        return Err(format!(
            "Unsupported managed Qwen ASR package: {}",
            asr.managed_qwen.package_id
        ));
    }
    if !["auto", "cpu", "gpu"].contains(&asr.managed_qwen.device.as_str()) {
        return Err(format!(
            "Unsupported managed Qwen ASR device: {}",
            asr.managed_qwen.device
        ));
    }
    if asr.backend != QWEN_MANAGED_BACKEND && !asr.service_settings.contains_key(&asr.backend) {
        return Err(format!(
            "Recognition service settings are missing for backend: {}",
            asr.backend
        ));
    }
    Ok(())
}

fn validate_osc(osc: &OscConfig, unitext: &UniTextConfig) -> Result<(), String> {
    if osc.port == 0 {
        return Err("OSC port must be between 1 and 65535".into());
    }
    if osc.inbound.port == 0 {
        return Err("OSC inbound port must be between 1 and 65535".into());
    }
    if osc.inbound.enabled {
        if osc.inbound.port == osc.port {
            return Err("OSC inbound port cannot match the VRChat OSC output port".into());
        }
        if unitext.enabled && osc.inbound.port == unitext.port {
            return Err("OSC inbound port cannot match the UniText OSC port".into());
        }
        if unitext.enabled && osc.inbound.port == 9001 {
            return Err("OSC inbound port 9001 conflicts with the UniText OSC receive port".into());
        }
    }
    if !["preferred_only", "round_robin", "all_languages"]
        .contains(&osc.translation_strategy.as_str())
    {
        return Err(format!(
            "Unsupported OSC translation strategy: {}",
            osc.translation_strategy
        ));
    }
    if osc.message_format == crate::chatbox::ChatboxMessageFormat::Custom {
        crate::chatbox::validate_custom_format(osc.custom_format.as_deref().unwrap_or_default())
            .map_err(|error| error.detail)?;
    } else if let Some(custom) = osc.custom_format.as_deref()
        && custom.chars().count() > crate::chatbox::MAX_CUSTOM_FORMAT_LENGTH
    {
        return Err(format!(
            "OSC custom format cannot exceed {} characters",
            crate::chatbox::MAX_CUSTOM_FORMAT_LENGTH
        ));
    }
    Ok(())
}

fn validate_unitext(unitext: &UniTextConfig) -> Result<(), String> {
    if unitext.port == 0 {
        return Err("UniText OSC port must be between 1 and 65535".into());
    }
    if !["auto", "lite", "compact", "balanced", "standard", "turbo"]
        .contains(&unitext.tier.as_str())
    {
        return Err(format!("Unsupported UniText tier: {}", unitext.tier));
    }
    if !(80..=500).contains(&unitext.interval_ms) {
        return Err("UniText interval_ms must be between 80 and 500".into());
    }
    if unitext.clear_after_seconds > 300 {
        return Err("UniText clear_after_seconds must be between 0 and 300".into());
    }
    Ok(())
}

fn validate_recognition_models(asr: &AsrConfig, vad: &VadConfig) -> Result<(), String> {
    for (service_id, settings) in &asr.service_settings {
        let (_, service) = providers::recognition_service(service_id)
            .ok_or_else(|| format!("Unsupported recognition service settings: {service_id}"))?;
        if !providers::recognition_model_supported(service, &settings.model) {
            return Err(format!(
                "Unsupported model for recognition service {service_id}: {}",
                settings.model
            ));
        }
        if let Some(max_chars) = service.context_max_chars {
            if settings.context.chars().count() > max_chars {
                return Err(format!(
                    "Recognition service {service_id} context cannot exceed {max_chars} characters"
                ));
            }
        } else if !settings.context.is_empty() {
            return Err(format!(
                "Recognition service {service_id} does not support context"
            ));
        }
    }
    if asr.backend == SERVICE_FUN_ASR_REALTIME && vad.silence_seconds < 0.2 {
        return Err("Fun-ASR realtime recognition requires at least 0.2 seconds of silence".into());
    }
    Ok(())
}

fn validate_vr_overlay(config: &VrOverlayConfig) -> Result<(), String> {
    if !["auto", "openvr", "openxr"].contains(&config.backend.as_str()) {
        return Err(format!(
            "Unsupported VR Overlay backend: {}",
            config.backend
        ));
    }
    if !["left", "right"].contains(&config.wrist_menu.hand.as_str()) {
        return Err("VR Overlay wrist menu hand must be left or right".into());
    }
    for (field, value) in [
        ("offset_x_m", config.wrist_menu.offset_x_m),
        ("offset_y_m", config.wrist_menu.offset_y_m),
        ("offset_z_m", config.wrist_menu.offset_z_m),
    ] {
        validate_range(&format!("VR Overlay wrist_menu {field}"), value, -0.5, 0.5)?;
    }
    for (field, value) in [
        ("pitch_deg", config.wrist_menu.pitch_deg),
        ("yaw_deg", config.wrist_menu.yaw_deg),
        ("roll_deg", config.wrist_menu.roll_deg),
    ] {
        validate_range(
            &format!("VR Overlay wrist_menu {field}"),
            value,
            -180.0,
            180.0,
        )?;
    }
    validate_range(
        "VR Overlay wrist_menu width_m",
        config.wrist_menu.width_m,
        0.12,
        0.50,
    )?;
    if !["wrist", "world"].contains(&config.wrist.anchor_mode.as_str()) {
        return Err("VR Overlay wrist anchor_mode must be wrist or world".into());
    }
    if !["preferred_only", "all_languages"].contains(&config.translation_display.as_str()) {
        return Err(format!(
            "Unsupported VR Overlay translation_display: {}",
            config.translation_display
        ));
    }
    validate_range(
        "VR Overlay headset offset_x_m",
        config.headset.offset_x_m,
        -2.0,
        2.0,
    )?;
    validate_range(
        "VR Overlay headset offset_y_m",
        config.headset.offset_y_m,
        -2.0,
        2.0,
    )?;
    validate_range(
        "VR Overlay headset distance_m",
        config.headset.distance_m,
        0.25,
        5.0,
    )?;
    validate_range(
        "VR Overlay headset pitch_deg",
        config.headset.pitch_deg,
        -90.0,
        90.0,
    )?;
    validate_range(
        "VR Overlay headset yaw_deg",
        config.headset.yaw_deg,
        -180.0,
        180.0,
    )?;
    validate_range(
        "VR Overlay headset roll_deg",
        config.headset.roll_deg,
        -180.0,
        180.0,
    )?;
    validate_range(
        "VR Overlay headset width_m",
        config.headset.width_m,
        0.25,
        3.0,
    )?;
    validate_range(
        "VR Overlay headset opacity",
        config.headset.opacity,
        0.10,
        1.0,
    )?;
    validate_range(
        "VR Overlay headset display_seconds",
        config.headset.display_seconds,
        1.0,
        30.0,
    )?;
    validate_range(
        "VR Overlay headset fade_seconds",
        config.headset.fade_seconds,
        0.0,
        5.0,
    )?;
    if config.headset.fade_seconds > config.headset.display_seconds {
        return Err("VR Overlay headset fade_seconds cannot exceed display_seconds".into());
    }
    if !(1..=4).contains(&config.headset.lines_per_language) {
        return Err("VR Overlay headset lines_per_language must be between 1 and 4".into());
    }
    if !(24..=96).contains(&config.headset.font_size_px) {
        return Err("VR Overlay headset font_size_px must be between 24 and 96".into());
    }
    validate_range(
        "VR Overlay headset background_opacity",
        config.headset.background_opacity,
        0.0,
        1.0,
    )?;

    if !["left", "right", "dominant"].contains(&config.wrist.hand.as_str()) {
        return Err(format!(
            "Unsupported VR Overlay wrist hand: {}",
            config.wrist.hand
        ));
    }
    if !["left", "right"].contains(&config.wrist.dominant_hand.as_str()) {
        return Err(format!(
            "Unsupported VR Overlay wrist dominant_hand: {}",
            config.wrist.dominant_hand
        ));
    }
    if !(3..=10).contains(&config.wrist.max_entries) {
        return Err("VR Overlay wrist max_entries must be between 3 and 10".into());
    }
    if config.wrist.idle_hide_seconds != 0 && !(5..=120).contains(&config.wrist.idle_hide_seconds) {
        return Err("VR Overlay wrist idle_hide_seconds must be 0 or between 5 and 120".into());
    }
    for (field, value) in [
        ("offset_x_m", config.wrist.offset_x_m),
        ("offset_y_m", config.wrist.offset_y_m),
        ("offset_z_m", config.wrist.offset_z_m),
    ] {
        validate_range(&format!("VR Overlay wrist {field}"), value, -0.5, 0.5)?;
    }
    for (field, value) in [
        ("pitch_deg", config.wrist.pitch_deg),
        ("yaw_deg", config.wrist.yaw_deg),
        ("roll_deg", config.wrist.roll_deg),
    ] {
        validate_range(&format!("VR Overlay wrist {field}"), value, -180.0, 180.0)?;
    }
    validate_range("VR Overlay wrist width_m", config.wrist.width_m, 0.10, 1.0)?;
    validate_range("VR Overlay wrist opacity", config.wrist.opacity, 0.10, 1.0)?;
    if !(18..=72).contains(&config.wrist.font_size_px) {
        return Err("VR Overlay wrist font_size_px must be between 18 and 72".into());
    }
    validate_range(
        "VR Overlay wrist background_opacity",
        config.wrist.background_opacity,
        0.0,
        1.0,
    )?;
    Ok(())
}

fn validate_range(label: &str, value: f32, minimum: f32, maximum: f32) -> Result<(), String> {
    if !(minimum..=maximum).contains(&value) {
        return Err(format!("{label} must be between {minimum} and {maximum}"));
    }
    Ok(())
}

fn validate_anki(anki: &AnkiConfig) -> Result<(), String> {
    if anki.port == 0 {
        return Err("AnkiConnect port must be between 1 and 65535".into());
    }
    for (label, value) in [
        ("deck", &anki.deck),
        ("note type", &anki.model),
        ("front field", &anki.front_field),
        ("back field", &anki.back_field),
    ] {
        if value.is_empty() || value.chars().count() > 100 {
            return Err(format!(
                "Anki {label} name must contain 1 to 100 characters"
            ));
        }
    }
    if anki.front_field == anki.back_field {
        return Err("Anki front and back fields cannot map to the same field".into());
    }
    Ok(())
}

fn validate_api_profiles(asr: &AsrConfig) -> Result<(), String> {
    use std::collections::HashSet;

    let mut ids = HashSet::new();
    let mut names = HashSet::new();
    for profile in &asr.api_profiles {
        let valid_id = !profile.id.is_empty()
            && profile.id.len() <= 64
            && profile
                .id
                .bytes()
                .all(|value| value.is_ascii_alphanumeric() || matches!(value, b'-' | b'_'));
        if !valid_id || !ids.insert(profile.id.as_str()) {
            return Err("An API profile ID is invalid or duplicated".into());
        }
        let name = profile.name.trim();
        if name.is_empty() || name.chars().count() > 50 {
            return Err("An API profile name must contain 1 to 50 characters".into());
        }
        if !names.insert((profile.provider.as_str(), name.to_lowercase())) {
            return Err(format!(
                "API profile names must be unique per provider: {name}"
            ));
        }
        providers::validate_profile(profile)?;
    }

    if asr.backend == QWEN_MANAGED_BACKEND {
        return Ok(());
    }
    let Some(active_id) = asr.active_profile_id.as_deref() else {
        return Ok(());
    };
    let active_profile = asr
        .api_profiles
        .iter()
        .find(|profile| profile.id == active_id)
        .ok_or_else(|| "The active recognition API profile does not exist".to_string())?;
    if !active_profile
        .enabled_capabilities
        .iter()
        .any(|capability| capability == CAPABILITY_SPEECH_TO_TEXT)
    {
        return Err("The active API profile has not enabled speech recognition".into());
    }
    providers::resolve_profile_service(active_profile, &asr.backend).map_err(|error| {
        format!("The active API profile does not match the recognition service: {error}")
    })?;
    Ok(())
}

fn validate_translation(
    translation: &TranslationConfig,
    profiles: &[ApiProfile],
    live_service: &str,
) -> Result<(), SettingsValidationError> {
    validate_translation_prompt(&translation.prompt)?;
    if !["disabled", "manual", "automatic"].contains(&translation.mode.as_str()) {
        return Err(format!("Unsupported translation mode: {}", translation.mode).into());
    }
    let use_default_model = translation.model_selection.use_default_model;
    validate_translation_targets(
        "speaker",
        &translation.speaker_targets,
        profiles,
        translation.mode != "disabled",
        use_default_model,
        if translation.mode == "automatic" {
            live_service
        } else {
            ""
        },
    )?;
    validate_translation_targets(
        "microphone",
        &translation.microphone_targets,
        profiles,
        translation.mode != "disabled",
        use_default_model,
        if translation.mode == "automatic" {
            live_service
        } else {
            ""
        },
    )
}

fn validate_translation_targets(
    source: &str,
    targets: &[TranslationTargetConfig],
    profiles: &[ApiProfile],
    require_profile: bool,
    use_default_model: bool,
    live_service: &str,
) -> Result<(), SettingsValidationError> {
    if !(1..=3).contains(&targets.len()) {
        return Err(
            format!("Translation {source} targets must contain between 1 and 3 entries").into(),
        );
    }
    let mut languages = HashSet::new();
    for (index, target) in targets.iter().enumerate() {
        if let Some(control) = &target.reasoning_control {
            control.validate()?;
        }
        if !languages.insert(target.target_language.to_ascii_lowercase()) {
            return Err(format!("Translation {source} target languages must be unique").into());
        }
        if providers::is_live_translation(live_service) && index == 0 {
            if !providers::is_valid_translation_language(&target.target_language) {
                return Err(format!(
                    "Invalid {source} translation target language: {}",
                    target.target_language
                )
                .into());
            }
            providers::validate_live_translation_language(live_service, &target.target_language)
                .map_err(|detail| {
                    SettingsValidationError::with_params(
                        "settings.translation_language_unsupported",
                        serde_json::json!({ "language": target.target_language }),
                        detail,
                    )
                })?;
            continue;
        }
        if target.use_default_model && !require_profile {
            // Optional followers retain unavailable bindings for diagnostics and provider repair.
            continue;
        }
        if target.backend == TranslationBackend::PlamoLocal {
            if target.profile_id.is_some() {
                return Err(format!(
                    "A local PLaMo translation binding cannot select an API profile for {}",
                    target.target_language
                )
                .into());
            }
            if target.model != crate::translation::MODEL_ID {
                return Err(format!(
                    "The local PLaMo translation model must be {}",
                    crate::translation::MODEL_ID
                )
                .into());
            }
            if target.thinking_enabled || target.reasoning_control.is_some() {
                return Err("Local PLaMo translation does not support reasoning controls".into());
            }
            if !crate::translation::plamo_supports_language(&target.target_language) {
                return Err(SettingsValidationError::with_params(
                    "settings.translation_language_unsupported",
                    serde_json::json!({ "language": target.target_language }),
                    format!(
                        "PLaMo does not support target language: {}",
                        target.target_language
                    ),
                ));
            }
            continue;
        }
        if !providers::is_valid_translation_language(&target.target_language) {
            return Err(format!(
                "Invalid {source} translation target language: {}",
                target.target_language
            )
            .into());
        }
        let Some(profile_id) = target.profile_id.as_deref() else {
            if require_profile {
                let code = if use_default_model {
                    "settings.translation_default_model_missing"
                } else {
                    "settings.translation_profile_missing"
                };
                return Err(SettingsValidationError::coded(
                    code,
                    format!(
                        "A translation API profile must be selected for {}",
                        target.target_language
                    ),
                ));
            }
            continue;
        };
        let profile = profiles
            .iter()
            .find(|profile| profile.id == profile_id)
            .ok_or_else(|| {
                SettingsValidationError::coded(
                    "settings.translation_profile_unavailable",
                    "The selected translation API profile does not exist",
                )
            })?;
        if !providers::supports_translation(profile) {
            return Err(SettingsValidationError::coded(
                "settings.translation_profile_unavailable",
                "The selected API profile does not support translation",
            ));
        }
        if !providers::supports_translation_language(profile, &target.target_language) {
            return Err(SettingsValidationError::with_params(
                "settings.translation_language_unsupported",
                serde_json::json!({ "language": target.target_language }),
                format!(
                    "The selected API profile does not support target language: {}",
                    target.target_language
                ),
            ));
        }
        if providers::supports_llm_models(profile) && target.model.trim().is_empty() {
            return Err(SettingsValidationError::coded(
                "settings.translation_model_required",
                "The LLM translation model cannot be empty",
            ));
        }
    }
    Ok(())
}

fn validate_translation_selection(
    name: &str,
    selection: &super::ModelSelection,
) -> Result<(), String> {
    if selection.use_default_model || selection.backend == TranslationBackend::Api {
        return Ok(());
    }
    if selection.profile_id.is_some() {
        return Err(format!(
            "{name} cannot select an API profile for local PLaMo translation"
        ));
    }
    if selection.model != crate::translation::MODEL_ID {
        return Err(format!(
            "{name}.model must be {}",
            crate::translation::MODEL_ID
        ));
    }
    Ok(())
}

fn validate_language_presets(
    presets: &[LanguagePreset],
    profiles: &[ApiProfile],
    live_service: &str,
    use_default_model: bool,
) -> Result<(), SettingsValidationError> {
    if presets.len() > 5 {
        return Err("Language presets cannot exceed 5 entries".into());
    }
    let mut ids = HashSet::new();
    let mut names = HashSet::new();
    for preset in presets {
        if uuid::Uuid::parse_str(&preset.id).is_err() || !ids.insert(preset.id.as_str()) {
            return Err("Language preset IDs must be unique UUIDs".into());
        }
        let name = preset.name.trim();
        if name.is_empty() || name.chars().count() > 40 || !names.insert(name.to_ascii_lowercase())
        {
            return Err(
                "Language preset names must be unique and contain 1 to 40 characters".into(),
            );
        }
        validate_asr_language_list(
            &preset.speaker_recognition_languages,
            "preset speaker recognition language",
        )?;
        validate_asr_language_list(
            &preset.microphone_recognition_languages,
            "preset microphone recognition language",
        )?;
        if !["disabled", "manual", "automatic"].contains(&preset.translation_mode.as_str()) {
            return Err(format!(
                "Unsupported preset translation mode: {}",
                preset.translation_mode
            )
            .into());
        }
        validate_translation_targets(
            "preset speaker",
            &preset.speaker_targets,
            profiles,
            preset.translation_mode != "disabled",
            use_default_model,
            if preset.translation_mode == "automatic" {
                live_service
            } else {
                ""
            },
        )?;
        validate_translation_targets(
            "preset microphone",
            &preset.microphone_targets,
            profiles,
            preset.translation_mode != "disabled",
            use_default_model,
            if preset.translation_mode == "automatic" {
                live_service
            } else {
                ""
            },
        )?;
        if !["preferred_only", "round_robin", "all_languages"]
            .contains(&preset.osc_translation_strategy.as_str())
        {
            return Err(format!(
                "Unsupported preset OSC translation strategy: {}",
                preset.osc_translation_strategy
            )
            .into());
        }
    }
    Ok(())
}

pub fn validate_translation_prompt(prompt: &TranslationPromptConfig) -> Result<(), String> {
    if prompt.system_prompt.chars().count() > 8_000 {
        return Err("Translation system prompt cannot exceed 8000 characters".into());
    }
    validate_prompt_variables(&prompt.system_prompt)?;
    if !(1..=50).contains(&prompt.max_messages) {
        return Err("Translation context max_messages must be between 1 and 50".into());
    }
    if !(200..=12_000).contains(&prompt.max_chars) {
        return Err("Translation context max_chars must be between 200 and 12000".into());
    }
    validate_glossary_entries(&prompt.glossary, "Translation glossary")?;
    Ok(())
}

pub fn validate_glossary(glossary: &GlossaryConfig) -> Result<(), String> {
    let mut source_ids = HashSet::new();
    for source in &glossary.sources {
        let id = match source {
            GlossarySource::Local {
                id,
                name,
                enabled: _,
                entries,
            } => {
                let name_length = name.chars().count();
                if name.trim().is_empty() || !(1..=100).contains(&name_length) {
                    return Err("Local glossary name must contain 1 to 100 characters".into());
                }
                validate_glossary_entries(entries, "Local glossary")?;
                id
            }
            GlossarySource::Subscription {
                id,
                url,
                display_name,
                enabled: _,
            } => {
                validate_glossary_source_url(url)?;
                if display_name
                    .as_ref()
                    .is_some_and(|name| name.chars().count() > 100)
                {
                    return Err(
                        "Glossary subscription display_name cannot exceed 100 characters".into(),
                    );
                }
                id
            }
        };
        let id = id.trim();
        if id.is_empty() {
            return Err("Glossary source id cannot be empty".into());
        }
        if !source_ids.insert(id) {
            return Err(format!("Glossary source id must be unique: {id}"));
        }
    }
    Ok(())
}

fn validate_glossary_entries(entries: &[GlossaryEntry], label: &str) -> Result<(), String> {
    if entries.len() > 500 {
        return Err(format!("{label} cannot exceed 500 entries"));
    }
    let mut keys = HashSet::new();
    for entry in entries {
        let source = entry.source.trim();
        if source.is_empty() || source.chars().count() > 200 || contains_control(source) {
            return Err(format!(
                "{label} source must contain 1 to 200 single-line characters"
            ));
        }
        if entry
            .target
            .as_deref()
            .is_some_and(|target| target.chars().count() > 200 || contains_control(target))
        {
            return Err(format!(
                "{label} target must contain at most 200 single-line characters"
            ));
        }
        let key = (
            if entry.case_sensitive {
                source.to_owned()
            } else {
                source.to_lowercase()
            },
            entry.case_sensitive,
        );
        if !keys.insert(key) {
            return Err(format!(
                "{label} contains a duplicate source term: {source}"
            ));
        }
    }
    Ok(())
}

fn contains_control(value: &str) -> bool {
    value.chars().any(char::is_control)
}

pub fn validate_glossary_source_url(value: &str) -> Result<(), String> {
    let value = value.trim();
    if value.is_empty() || value.len() > 2_048 {
        return Err("Glossary source URL must contain 1 to 2048 characters".into());
    }
    let url =
        reqwest::Url::parse(value).map_err(|_| "Glossary source URL is invalid".to_string())?;
    if url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err("Glossary source URL cannot contain credentials or a fragment".into());
    }
    if url.scheme() == "https" {
        return Ok(());
    }
    let loopback = url.host_str().is_some_and(|host| {
        host.eq_ignore_ascii_case("localhost")
            || host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|address| address.is_loopback())
    });
    if url.scheme() == "http" && loopback {
        return Ok(());
    }
    Err("Glossary source URL must use HTTPS, except for loopback HTTP addresses".into())
}

fn validate_prompt_variables(template: &str) -> Result<(), String> {
    let mut start = None;
    for (index, character) in template.char_indices() {
        match (character, start) {
            ('{', None) => start = Some(index + 1),
            ('{', Some(_)) => {
                return Err("Translation system prompt contains a nested variable".into());
            }
            ('}', Some(variable_start)) => {
                let variable = &template[variable_start..index];
                if !["source_language", "target_language", "glossary", "context"]
                    .contains(&variable)
                {
                    return Err(format!(
                        "Unsupported translation prompt variable: {{{variable}}}"
                    ));
                }
                start = None;
            }
            ('}', None) => {
                return Err("Translation system prompt contains an unmatched closing brace".into());
            }
            _ => {}
        }
    }
    if start.is_some() {
        return Err("Translation system prompt contains an unclosed variable".into());
    }
    Ok(())
}

#[cfg(test)]
mod player_audio_tests {
    use super::*;

    #[test]
    fn player_audio_is_opt_in_and_validates_resource_limits() {
        let mut audio = AudioConfig::default();
        let vad = VadConfig::default();
        assert_eq!(audio.output.mode, "system");
        assert_eq!(audio.output.player_audio.max_players, 0);
        audio.output.player_audio.enabled = true;
        audio.output.mode = "vrchat_players".into();
        assert!(validate_audio(&audio, &vad).is_ok());
        audio.output.player_audio.max_players = 9;
        assert!(validate_audio(&audio, &vad).is_err());
        audio.output.player_audio.max_players = 0;
        assert!(validate_audio(&audio, &vad).is_ok());
        audio.output.player_audio.max_players = 4;
        audio.output.device_id = Some(1);
        assert!(validate_audio(&audio, &vad).is_err());
        audio.output.device_id = None;
        #[cfg(windows)]
        {
            audio.output.player_audio.pagefile_drive = "C:\\other".into();
            assert!(validate_audio(&audio, &vad).is_err());
        }
        let legacy: AudioConfig = serde_json::from_str(r#"{"output":{"mode":"vrchat"}}"#).unwrap();
        assert_eq!(
            legacy.output.player_audio,
            crate::config::PlayerAudioConfig::default()
        );
    }
}
