use std::collections::BTreeMap;

use serde::{Deserialize, Serialize, de};
use serde_json::Value;

use super::common::*;

pub type ImageGenerationWireModel = OpenAiWireModel<ImageGenerationRequest, ImagesResponse>;
pub type ImageGenerationStreamWireModel =
    OpenAiWireModel<ImageGenerationRequest, ImageGenerationStreamEvent>;
pub type ImageEditWireModel = OpenAiWireModel<ImageEditRequest, ImagesResponse>;
pub type ImageEditStreamWireModel = OpenAiWireModel<ImageEditRequest, ImageEditStreamEvent>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImageGenerationRequest {
    pub prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<ImageBackground>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<OpenAiModelId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub moderation: Option<ImageModeration>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub n: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_compression: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_format: Option<ImageOutputFormat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub partial_images: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality: Option<ImageQuality>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<ImageResponseFormat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<ImageSize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub style: Option<ImageStyle>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImageEditRequest {
    pub images: Vec<ImageReference>,
    pub prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<ImageBackground>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_fidelity: Option<ImageInputFidelity>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mask: Option<ImageReference>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<OpenAiModelId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub moderation: Option<ImageModeration>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub n: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_compression: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_format: Option<ImageOutputFormat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub partial_images: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality: Option<ImageQuality>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<ImageSize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ImageReference {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_url: Option<String>,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: Extra,
}

impl<'de> Deserialize<'de> for ImageReference {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawImageReference {
            file_id: Option<String>,
            image_url: Option<String>,
            #[serde(default, flatten)]
            extra: Extra,
        }

        let raw = RawImageReference::deserialize(deserializer)?;
        match (raw.file_id.is_some(), raw.image_url.is_some()) {
            (true, false) | (false, true) => Ok(Self {
                file_id: raw.file_id,
                image_url: raw.image_url,
                extra: raw.extra,
            }),
            (true, true) => Err(de::Error::custom(
                "image reference must contain exactly one of file_id or image_url",
            )),
            (false, false) => Err(de::Error::custom(
                "image reference must contain file_id or image_url",
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImagesResponse {
    pub created: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<ImageResponseBackground>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Vec<Image>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_format: Option<ImageOutputFormat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality: Option<ImageResponseQuality>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<ImageResponseSize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<ImageUsage>,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Image {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub b64_json: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revised_prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImageUsage {
    pub input_tokens: u32,
    pub input_tokens_details: ImageTokenDetails,
    pub output_tokens: u32,
    pub total_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens_details: Option<ImageTokenDetails>,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImageTokenDetails {
    pub image_tokens: u32,
    pub text_tokens: u32,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum ImageStreamEvent {
    Known(KnownImageStreamEvent),
    Unknown(UnknownImageStreamEvent),
}

impl<'de> Deserialize<'de> for ImageStreamEvent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        match image_stream_event_type::<D::Error>(&value)? {
            Some(ImageStreamEventType::Known(_)) => serde_json::from_value(value)
                .map(Self::Known)
                .map_err(de::Error::custom),
            Some(ImageStreamEventType::Unknown(_)) | None => serde_json::from_value(value)
                .map(Self::Unknown)
                .map_err(de::Error::custom),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum KnownImageStreamEvent {
    #[serde(rename = "image_generation.partial_image")]
    ImageGenerationPartialImage {
        b64_json: String,
        partial_image_index: u32,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: Extra,
    },
    #[serde(rename = "image_generation.completed")]
    ImageGenerationCompleted {
        b64_json: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        usage: Option<ImageUsage>,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: Extra,
    },
    #[serde(rename = "image_edit.partial_image")]
    ImageEditPartialImage {
        b64_json: String,
        partial_image_index: u32,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: Extra,
    },
    #[serde(rename = "image_edit.completed")]
    ImageEditCompleted {
        b64_json: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        usage: Option<ImageUsage>,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: Extra,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum ImageGenerationStreamEvent {
    Known(KnownImageGenerationStreamEvent),
    Unknown(UnknownImageStreamEvent),
}

impl<'de> Deserialize<'de> for ImageGenerationStreamEvent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        match image_stream_event_type::<D::Error>(&value)? {
            Some(ImageStreamEventType::Known(
                ImageStreamEventTypeKnown::ImageGenerationPartialImage
                | ImageStreamEventTypeKnown::ImageGenerationCompleted,
            )) => serde_json::from_value(value)
                .map(Self::Known)
                .map_err(de::Error::custom),
            Some(ImageStreamEventType::Known(_)) => Err(de::Error::custom(
                "known image edit stream event cannot deserialize as image generation stream event",
            )),
            Some(ImageStreamEventType::Unknown(_)) | None => serde_json::from_value(value)
                .map(Self::Unknown)
                .map_err(de::Error::custom),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum KnownImageGenerationStreamEvent {
    #[serde(rename = "image_generation.partial_image")]
    PartialImage {
        b64_json: String,
        partial_image_index: u32,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: Extra,
    },
    #[serde(rename = "image_generation.completed")]
    Completed {
        b64_json: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        usage: Option<ImageUsage>,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: Extra,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum ImageEditStreamEvent {
    Known(KnownImageEditStreamEvent),
    Unknown(UnknownImageStreamEvent),
}

impl<'de> Deserialize<'de> for ImageEditStreamEvent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        match image_stream_event_type::<D::Error>(&value)? {
            Some(ImageStreamEventType::Known(
                ImageStreamEventTypeKnown::ImageEditPartialImage
                | ImageStreamEventTypeKnown::ImageEditCompleted,
            )) => serde_json::from_value(value)
                .map(Self::Known)
                .map_err(de::Error::custom),
            Some(ImageStreamEventType::Known(_)) => Err(de::Error::custom(
                "known image generation stream event cannot deserialize as image edit stream event",
            )),
            Some(ImageStreamEventType::Unknown(_)) | None => serde_json::from_value(value)
                .map(Self::Unknown)
                .map_err(de::Error::custom),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum KnownImageEditStreamEvent {
    #[serde(rename = "image_edit.partial_image")]
    PartialImage {
        b64_json: String,
        partial_image_index: u32,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: Extra,
    },
    #[serde(rename = "image_edit.completed")]
    Completed {
        b64_json: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        usage: Option<ImageUsage>,
        #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
        extra: Extra,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnknownImageStreamEvent {
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub type_: Option<ImageStreamEventType>,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: Extra,
}

fn image_stream_event_type<E>(value: &Value) -> Result<Option<ImageStreamEventType>, E>
where
    E: de::Error,
{
    let Some(type_name) = value.get("type").and_then(Value::as_str) else {
        return Ok(None);
    };

    serde_json::from_value(Value::String(type_name.to_owned()))
        .map(Some)
        .map_err(de::Error::custom)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn image_generation_stream_event_models_partial_image() {
        let event: ImageGenerationStreamEvent = serde_json::from_value(json!({
            "type": "image_generation.partial_image",
            "b64_json": "...",
            "partial_image_index": 0
        }))
        .expect("generation partial image event should deserialize");

        assert!(matches!(
            event,
            ImageGenerationStreamEvent::Known(KnownImageGenerationStreamEvent::PartialImage { .. })
        ));
    }

    #[test]
    fn image_edit_stream_event_models_completed_image() {
        let event: ImageEditStreamEvent = serde_json::from_value(json!({
            "type": "image_edit.completed",
            "b64_json": "...",
            "usage": {
                "input_tokens": 50,
                "input_tokens_details": { "text_tokens": 10, "image_tokens": 40 },
                "output_tokens": 50,
                "total_tokens": 100
            }
        }))
        .expect("edit completed event should deserialize");

        let ImageEditStreamEvent::Known(KnownImageEditStreamEvent::Completed { usage, .. }) = event
        else {
            panic!("expected image_edit completed event");
        };
        assert_eq!(usage.expect("usage").total_tokens, 100);
    }

    #[test]
    fn image_stream_event_keeps_unknown_event_extensible() {
        let event: ImageStreamEvent = serde_json::from_value(json!({
            "type": "image_future.event",
            "payload": { "x": 1 }
        }))
        .expect("unknown image stream event should deserialize");

        assert!(matches!(event, ImageStreamEvent::Unknown(_)));
    }

    #[test]
    fn image_stream_event_rejects_invalid_known_event_shape() {
        let result = serde_json::from_value::<ImageStreamEvent>(json!({
            "type": "image_generation.partial_image",
            "b64_json": "..."
        }));

        assert!(result.is_err());
    }

    #[test]
    fn image_generation_stream_rejects_edit_event_type() {
        let result = serde_json::from_value::<ImageGenerationStreamEvent>(json!({
            "type": "image_edit.completed",
            "b64_json": "..."
        }));

        assert!(result.is_err());
    }

    #[test]
    fn images_response_uses_response_only_image_enums() {
        let response: ImagesResponse = serde_json::from_value(json!({
            "created": 1,
            "background": "transparent",
            "output_format": "png",
            "quality": "high",
            "size": "1024x1536",
            "data": []
        }))
        .expect("images response should deserialize");

        assert_eq!(
            response.background,
            Some(ImageResponseBackground::Transparent)
        );
        assert_eq!(response.quality, Some(ImageResponseQuality::High));
        assert_eq!(response.size, Some(ImageResponseSize::Size1024By1536));
    }

    #[test]
    fn image_request_rejects_undocumented_closed_enum_values() {
        let invalid_background = serde_json::from_value::<ImageGenerationRequest>(json!({
            "prompt": "a city skyline",
            "background": "gradient"
        }));
        assert!(invalid_background.is_err());

        let invalid_response_format = serde_json::from_value::<ImageGenerationRequest>(json!({
            "prompt": "a city skyline",
            "response_format": "bytes"
        }));
        assert!(invalid_response_format.is_err());

        let invalid_style = serde_json::from_value::<ImageGenerationRequest>(json!({
            "prompt": "a city skyline",
            "style": "cinematic"
        }));
        assert!(invalid_style.is_err());
    }

    #[test]
    fn image_size_keeps_documented_string_fallback() {
        let request: ImageGenerationRequest = serde_json::from_value(json!({
            "prompt": "a city skyline",
            "size": "1536x864"
        }))
        .expect("arbitrary documented image size string should deserialize");

        assert!(matches!(
            request.size,
            Some(ImageSize::Unknown(value)) if value == "1536x864"
        ));
    }

    #[test]
    fn image_edit_reference_requires_exactly_one_source() {
        let request: ImageEditRequest = serde_json::from_value(json!({
            "prompt": "make it brighter",
            "images": [{ "image_url": "https://example.com/source.png" }],
            "mask": { "file_id": "file_mask" }
        }))
        .expect("image references with one source should deserialize");

        assert_eq!(
            request.images[0].image_url.as_deref(),
            Some("https://example.com/source.png")
        );
        assert_eq!(
            request.mask.expect("mask").file_id.as_deref(),
            Some("file_mask")
        );

        let both = serde_json::from_value::<ImageEditRequest>(json!({
            "prompt": "make it brighter",
            "images": [{
                "file_id": "file_123",
                "image_url": "https://example.com/source.png"
            }]
        }));
        assert!(both.is_err());

        let neither = serde_json::from_value::<ImageEditRequest>(json!({
            "prompt": "make it brighter",
            "images": [{}]
        }));
        assert!(neither.is_err());
    }
}
