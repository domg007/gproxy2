//! OpenAI -> Gemini embedding transforms.

use crate::protocol::{gemini, openai};
use crate::transform::{TransformContext, TransformError};

use super::u32_to_i32;

pub fn request(
    input: openai::EmbeddingRequest,
    ctx: &TransformContext,
) -> Result<gemini::BatchEmbedContentsRequest, TransformError> {
    Ok(gemini::BatchEmbedContentsRequest {
        requests: requests(input, ctx)?,
        extra: Default::default(),
    })
}

pub fn single_request(
    input: openai::EmbeddingRequest,
    ctx: &TransformContext,
) -> Result<gemini::EmbedContentRequest, TransformError> {
    let mut requests = requests(input, ctx)?;
    if requests.len() != 1 {
        return Err(TransformError::InvalidInput {
            reason: "single Gemini embedding request requires exactly one OpenAI input".to_owned(),
        });
    }
    Ok(requests.remove(0))
}

pub fn response(
    input: openai::EmbeddingResponse,
    _: &TransformContext,
) -> Result<gemini::BatchEmbedContentsResponse, TransformError> {
    Ok(gemini::BatchEmbedContentsResponse {
        embeddings: input
            .data
            .into_iter()
            .map(content_embedding)
            .collect::<Result<Vec<_>, _>>()?,
        usage_metadata: Some(usage_metadata(input.usage)?),
        extra: Default::default(),
    })
}

pub fn single_response(
    input: openai::EmbeddingResponse,
    ctx: &TransformContext,
) -> Result<gemini::EmbedContentResponse, TransformError> {
    let mut response = response(input, ctx)?;
    if response.embeddings.len() != 1 {
        return Err(TransformError::InvalidInput {
            reason: "single Gemini embedding response requires exactly one OpenAI embedding"
                .to_owned(),
        });
    }

    Ok(gemini::EmbedContentResponse {
        embedding: Some(response.embeddings.remove(0)),
        usage_metadata: response.usage_metadata,
        extra: Default::default(),
    })
}

fn requests(
    input: openai::EmbeddingRequest,
    _: &TransformContext,
) -> Result<Vec<gemini::EmbedContentRequest>, TransformError> {
    reject_base64_encoding(input.encoding_format)?;
    if input.user.is_some() {
        return Err(TransformError::UnsupportedField {
            field: "user",
            reason: "Gemini embedding request has no user identifier field",
        });
    }

    let model = model_name(&input.model)?;
    let output_dimensionality = input
        .dimensions
        .map(|dimensions| u32_to_i32(dimensions, "dimensions"))
        .transpose()?;

    input
        .input
        .into_strings()?
        .into_iter()
        .map(|text| {
            Ok(gemini::EmbedContentRequest {
                model: Some(model.clone()),
                content: Some(text_content(text)),
                task_type: None,
                title: None,
                output_dimensionality,
                extra: Default::default(),
            })
        })
        .collect()
}

fn reject_base64_encoding(
    encoding_format: Option<openai::EmbeddingEncodingFormat>,
) -> Result<(), TransformError> {
    match encoding_format {
        Some(openai::EmbeddingEncodingFormat::Base64) => Err(TransformError::UnsupportedField {
            field: "encoding_format",
            reason: "Gemini embedding response does not support OpenAI base64 embeddings",
        }),
        _ => Ok(()),
    }
}

fn model_name(model: &openai::OpenAiModelId) -> Result<String, TransformError> {
    let value = serde_json::to_value(model).map_err(|error| TransformError::Serialization {
        reason: error.to_string(),
    })?;
    value
        .as_str()
        .map(str::to_owned)
        .ok_or(TransformError::InvalidInput {
            reason: "model did not serialize as a string".to_owned(),
        })
}

fn text_content(text: String) -> gemini::Content {
    gemini::Content {
        parts: vec![gemini::Part {
            thought: None,
            thought_signature: None,
            part_metadata: None,
            media_resolution: None,
            data: Some(gemini::PartData::Text { text }),
            metadata: None,
            extra: Default::default(),
        }],
        role: None,
        extra: Default::default(),
    }
}

fn content_embedding(input: openai::Embedding) -> Result<gemini::ContentEmbedding, TransformError> {
    Ok(gemini::ContentEmbedding {
        values: input
            .embedding
            .into_iter()
            .map(f64_to_f32)
            .collect::<Result<Vec<_>, _>>()?,
        shape: Vec::new(),
        extra: Default::default(),
    })
}

fn f64_to_f32(value: f64) -> Result<f32, TransformError> {
    if !value.is_finite() || value < f32::MIN as f64 || value > f32::MAX as f64 {
        return Err(TransformError::LossyField {
            field: "embedding",
            reason: "embedding value cannot be represented as f32",
        });
    }
    Ok(value as f32)
}

fn usage_metadata(
    input: openai::EmbeddingUsage,
) -> Result<gemini::EmbeddingUsageMetadata, TransformError> {
    Ok(gemini::EmbeddingUsageMetadata {
        total_token_count: Some(u32_to_i32(input.total_tokens, "usage.total_tokens")?),
        input_token_count: Some(u32_to_i32(input.prompt_tokens, "usage.prompt_tokens")?),
        prompt_tokens_details: Vec::new(),
        extra: Default::default(),
    })
}

trait EmbeddingInputExt {
    fn into_strings(self) -> Result<Vec<String>, TransformError>;
}

impl EmbeddingInputExt for openai::EmbeddingInput {
    fn into_strings(self) -> Result<Vec<String>, TransformError> {
        match self {
            Self::Text(text) => Ok(vec![text]),
            Self::TextList(values) => Ok(values),
            Self::TokenList(_) | Self::TokenLists(_) => Err(TransformError::UnsupportedField {
                field: "input",
                reason: "Gemini embedding content cannot represent OpenAI token id inputs",
            }),
        }
    }
}
