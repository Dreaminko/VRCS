use std::collections::HashSet;

use super::*;
use crate::providers::{self, CAPABILITY_SPEECH_TO_TEXT, SERVICE_FUN_ASR_REALTIME};

impl VadConfig {
    pub fn validate(&self) -> Result<(), String> {
        if !(0.1..=2.0).contains(&self.silence_seconds) {
            return Err("VAD silence_seconds must be between 0.1 and 2.0".into());
        }
        if !(1.0..=30.0).contains(&self.max_speech_seconds) {
            return Err("VAD max_speech_seconds must be between 1.0 and 30.0".into());
        }
        Ok(())
    }
}

const ASR_LANGUAGES: [&str; 8] = ["auto", "en", "ja", "zh", "ko", "es", "fr", "de"];

const ASR_DEVICES: [&str; 3] = ["auto", "cpu", "cuda"];
const ASR_COMPUTE_TYPES: [&str; 1] = ["int8"];
const CLOUD_FAILURE_POLICIES: [&str; 2] = ["reconnect", "local"];

impl AppConfig {
    /// 配置文件与 PUT /api/settings 共用的完整结构校验。
    pub fn validate_settings(&self) -> Result<(), String> {
        validate_runtime(&self.server, &self.storage, &self.external_api, &self.vrcx)?;
        validate_audio(&self.audio, &self.vad)?;
        validate_recognition_options(&self.asr)?;
        validate_api_profiles(&self.asr)?;
        validate_glossary(&self.glossary)?;
        validate_translation(&self.translation, &self.asr.api_profiles)?;
        validate_language_presets(&self.language_presets, &self.asr.api_profiles)?;
        validate_osc(&self.osc)?;
        validate_recognition_models(&self.asr, &self.vad)?;
        validate_anki(&self.anki)?;
        validate_vr_overlay(&self.vr_overlay)?;
        Ok(())
    }
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
        "vrchat" | "disabled" if audio.output.device_id.is_some() => {
            return Err(
                "VRChat or disabled output mode cannot specify a system output device".into(),
            );
        }
        "vrchat" | "disabled" => {}
        other => return Err(format!("Unsupported output mode: {other}")),
    }
    if !(-80.0..=-10.0).contains(&audio.output.trigger_threshold_dbfs) {
        return Err("Output trigger_threshold_dbfs must be between -80 and -10".into());
    }
    match audio.microphone.mode.as_str() {
        "device" if audio.microphone.device_id.is_none() => {
            return Err("A device must be selected in microphone device mode".into());
        }
        "device" => {}
        "default" | "disabled" if audio.microphone.device_id.is_some() => {
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
    if asr.backend != "local_whisper" && providers::recognition_service(&asr.backend).is_none() {
        return Err(format!("Unsupported recognition backend: {}", asr.backend));
    }
    if !ASR_LANGUAGES.contains(&asr.language.as_str()) {
        return Err(format!(
            "Unsupported recognition language: {}",
            asr.language
        ));
    }
    if !ASR_DEVICES.contains(&asr.local.device.as_str()) {
        return Err(format!(
            "Unsupported recognition device: {}",
            asr.local.device
        ));
    }
    if !ASR_COMPUTE_TYPES.contains(&asr.local.compute_type.as_str()) {
        return Err(format!(
            "Unsupported compute type: {}",
            asr.local.compute_type
        ));
    }
    if !CLOUD_FAILURE_POLICIES.contains(&asr.cloud_failure_policy.as_str()) {
        return Err(format!(
            "Unsupported cloud failure policy: {}",
            asr.cloud_failure_policy
        ));
    }
    if asr.backend != "local_whisper" && !asr.service_settings.contains_key(&asr.backend) {
        return Err(format!(
            "Recognition service settings are missing for backend: {}",
            asr.backend
        ));
    }
    Ok(())
}

fn validate_osc(osc: &OscConfig) -> Result<(), String> {
    if osc.port == 0 {
        return Err("OSC port must be between 1 and 65535".into());
    }
    if !["preferred_only", "round_robin", "all_languages"]
        .contains(&osc.translation_strategy.as_str())
    {
        return Err(format!(
            "Unsupported OSC translation strategy: {}",
            osc.translation_strategy
        ));
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
    if !["preferred_only", "all_languages"].contains(&config.translation_display.as_str()) {
        return Err(format!(
            "Unsupported VR Overlay translation_display: {}",
            config.translation_display
        ));
    }
    validate_content_mode("VR Overlay headset", &config.headset.content_mode)?;
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
    validate_content_mode("VR Overlay wrist", &config.wrist.content_mode)?;
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

fn validate_content_mode(label: &str, value: &str) -> Result<(), String> {
    if !["original", "translation", "bilingual"].contains(&value) {
        return Err(format!("Unsupported {label} content_mode: {value}"));
    }
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

    if asr.backend == "local_whisper" {
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
) -> Result<(), String> {
    validate_translation_prompt(&translation.prompt)?;
    if !["disabled", "manual", "automatic"].contains(&translation.mode.as_str()) {
        return Err(format!(
            "Unsupported translation mode: {}",
            translation.mode
        ));
    }
    validate_translation_targets(
        "speaker",
        &translation.speaker_targets,
        profiles,
        translation.mode != "disabled",
    )?;
    validate_translation_targets(
        "microphone",
        &translation.microphone_targets,
        profiles,
        translation.mode != "disabled",
    )
}

fn validate_translation_targets(
    source: &str,
    targets: &[TranslationTargetConfig],
    profiles: &[ApiProfile],
    require_profile: bool,
) -> Result<(), String> {
    if !(1..=3).contains(&targets.len()) {
        return Err(format!(
            "Translation {source} targets must contain between 1 and 3 entries"
        ));
    }
    let mut languages = HashSet::new();
    for target in targets {
        if !providers::is_valid_translation_language(&target.target_language) {
            return Err(format!(
                "Invalid {source} translation target language: {}",
                target.target_language
            ));
        }
        if !languages.insert(target.target_language.to_ascii_lowercase()) {
            return Err(format!(
                "Translation {source} target languages must be unique"
            ));
        }
        let Some(profile_id) = target.profile_id.as_deref() else {
            if require_profile {
                return Err(format!(
                    "A translation API profile must be selected for {}",
                    target.target_language
                ));
            }
            continue;
        };
        let profile = profiles
            .iter()
            .find(|profile| profile.id == profile_id)
            .ok_or_else(|| "The selected translation API profile does not exist".to_string())?;
        if !providers::supports_translation(profile) {
            return Err("The selected API profile does not support translation".into());
        }
        if !providers::supports_translation_language(profile, &target.target_language) {
            return Err(format!(
                "The selected API profile does not support target language: {}",
                target.target_language
            ));
        }
        if providers::supports_llm_models(profile) && target.model.trim().is_empty() {
            return Err("The LLM translation model cannot be empty".into());
        }
    }
    Ok(())
}

fn validate_language_presets(
    presets: &[LanguagePreset],
    profiles: &[ApiProfile],
) -> Result<(), String> {
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
        if !ASR_LANGUAGES.contains(&preset.recognition_language.as_str()) {
            return Err(format!(
                "Unsupported preset recognition language: {}",
                preset.recognition_language
            ));
        }
        if !["disabled", "manual", "automatic"].contains(&preset.translation_mode.as_str()) {
            return Err(format!(
                "Unsupported preset translation mode: {}",
                preset.translation_mode
            ));
        }
        validate_translation_targets(
            "preset speaker",
            &preset.speaker_targets,
            profiles,
            preset.translation_mode != "disabled",
        )?;
        validate_translation_targets(
            "preset microphone",
            &preset.microphone_targets,
            profiles,
            preset.translation_mode != "disabled",
        )?;
        if !["preferred_only", "round_robin", "all_languages"]
            .contains(&preset.osc_translation_strategy.as_str())
        {
            return Err(format!(
                "Unsupported preset OSC translation strategy: {}",
                preset.osc_translation_strategy
            ));
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
                return Err("Translation system prompt contains a nested variable".into())
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
                return Err("Translation system prompt contains an unmatched closing brace".into())
            }
            _ => {}
        }
    }
    if start.is_some() {
        return Err("Translation system prompt contains an unclosed variable".into());
    }
    Ok(())
}
