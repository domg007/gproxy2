use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::common::*;

pub type CreateVideoWireModel = OpenAiWireModel<CreateVideoRequestBody, VideoObject>;
pub type RetrieveVideoWireModel = OpenAiWireModel<RetrieveVideoPath, VideoObject>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateVideoRequestBody {
    pub prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_reference: Option<VideoInputReference>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<VideoModelId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seconds: Option<VideoSeconds>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<VideoSize>,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VideoInputReference {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_url: Option<String>,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RetrieveVideoPath {
    pub video_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RetrieveVideoContentRequest {
    pub video_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variant: Option<VideoContentVariant>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VideoObject {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<u64>,
    pub created_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<VideoCreateError>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<u64>,
    pub model: VideoModelId,
    pub object: VideoObjectType,
    pub progress: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality: Option<VideoQuality>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remixed_from_video_id: Option<String>,
    pub seconds: String,
    pub size: VideoSize,
    pub status: VideoStatus,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VideoCreateError {
    pub code: String,
    pub message: String,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: Extra,
}

macro_rules! extensible_video_enum {
    ($outer:ident, $known:ident { $($variant:ident => $wire:literal),+ $(,)? }) => {
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(untagged)]
        pub enum $outer {
            Known($known),
            Unknown(String),
        }

        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
        pub enum $known {
            $(
                #[serde(rename = $wire)]
                $variant,
            )+
        }
    };
}

extensible_video_enum!(VideoModelId, VideoModelIdKnown {
    Sora2 => "sora-2",
    Sora2Pro => "sora-2-pro",
    Sora220251006 => "sora-2-2025-10-06",
    Sora2Pro20251006 => "sora-2-pro-2025-10-06",
    Sora220251208 => "sora-2-2025-12-08",
});

extensible_video_enum!(VideoSeconds, VideoSecondsKnown {
    Four => "4",
    Eight => "8",
    Twelve => "12",
});

extensible_video_enum!(VideoSize, VideoSizeKnown {
    Size720By1280 => "720x1280",
    Size1280By720 => "1280x720",
    Size1024By1792 => "1024x1792",
    Size1792By1024 => "1792x1024",
});

extensible_video_enum!(VideoQuality, VideoQualityKnown {
    Standard => "standard",
});

extensible_video_enum!(VideoContentVariant, VideoContentVariantKnown {
    Video => "video",
    Thumbnail => "thumbnail",
    Spritesheet => "spritesheet",
});

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum VideoStatus {
    #[serde(rename = "queued")]
    Queued,
    #[serde(rename = "in_progress")]
    InProgress,
    #[serde(rename = "completed")]
    Completed,
    #[serde(rename = "failed")]
    Failed,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn create_video_request_models_known_parameters() {
        let request: CreateVideoRequestBody = serde_json::from_value(json!({
            "prompt": "A calico cat playing a piano on stage",
            "input_reference": { "file_id": "file_123" },
            "model": "sora-2",
            "seconds": "8",
            "size": "1024x1792"
        }))
        .expect("video request should deserialize");

        assert!(matches!(
            request.model,
            Some(VideoModelId::Known(VideoModelIdKnown::Sora2))
        ));
        assert!(matches!(
            request.seconds,
            Some(VideoSeconds::Known(VideoSecondsKnown::Eight))
        ));
        assert!(matches!(
            request.size,
            Some(VideoSize::Known(VideoSizeKnown::Size1024By1792))
        ));
    }

    #[test]
    fn video_response_models_queued_job_and_example_quality() {
        let response: VideoObject = serde_json::from_value(json!({
            "id": "video_123",
            "object": "video",
            "model": "sora-2",
            "status": "queued",
            "progress": 0,
            "created_at": 1712697600,
            "size": "1024x1792",
            "seconds": "8",
            "quality": "standard"
        }))
        .expect("video response should deserialize");

        assert_eq!(response.object, VideoObjectType::Video);
        assert_eq!(response.status, VideoStatus::Queued);
        assert!(matches!(
            response.quality,
            Some(VideoQuality::Known(VideoQualityKnown::Standard))
        ));
    }

    #[test]
    fn retrieve_video_content_request_models_variant_query() {
        let request: RetrieveVideoContentRequest = serde_json::from_value(json!({
            "video_id": "video_123",
            "variant": "thumbnail"
        }))
        .expect("video content request should deserialize");

        assert!(matches!(
            request.variant,
            Some(VideoContentVariant::Known(
                VideoContentVariantKnown::Thumbnail
            ))
        ));
    }
}
