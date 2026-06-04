use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::common::ExtraFields;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SpeechConfig {
    pub voice_config: Option<VoiceConfig>,
    pub multi_speaker_voice_config: Option<MultiSpeakerVoiceConfig>,
    pub language_code: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty", flatten)]
    pub extra: ExtraFields,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct VoiceConfig {
    pub prebuilt_voice_config: Option<PrebuiltVoiceConfig>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty", flatten)]
    pub extra: ExtraFields,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PrebuiltVoiceConfig {
    pub voice_name: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty", flatten)]
    pub extra: ExtraFields,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MultiSpeakerVoiceConfig {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub speaker_voice_configs: Vec<SpeakerVoiceConfig>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty", flatten)]
    pub extra: ExtraFields,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SpeakerVoiceConfig {
    pub speaker: Option<String>,
    pub voice_config: Option<VoiceConfig>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty", flatten)]
    pub extra: ExtraFields,
}
