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
    Ok(requests.pop().unwrap_or_default())
}

pub fn response(
    input: openai::EmbeddingResponse,
    _: &TransformContext,
) -> Result<gemini::BatchEmbedContentsResponse, TransformError> {
    Ok(gemini::BatchEmbedContentsResponse {
        embeddings: input.data.into_iter().map(content_embedding).collect(),
        usage_metadata: Some(usage_metadata(input.usage)?),
        extra: Default::default(),
    })
}

pub fn single_response(
    input: openai::EmbeddingResponse,
    ctx: &TransformContext,
) -> Result<gemini::EmbedContentResponse, TransformError> {
    let mut response = response(input, ctx)?;

    Ok(gemini::EmbedContentResponse {
        embedding: response.embeddings.pop(),
        usage_metadata: response.usage_metadata,
        extra: Default::default(),
    })
}

fn requests(
    input: openai::EmbeddingRequest,
    _: &TransformContext,
) -> Result<Vec<gemini::EmbedContentRequest>, TransformError> {
    let model = model_name(&input.model);
    let output_dimensionality = input.dimensions.map(u32_to_i32);

    input
        .input
        .into_strings()
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

fn model_name(model: &openai::OpenAiModelId) -> String {
    let Ok(value) = serde_json::to_value(model) else {
        return String::new();
    };
    value.as_str().map(str::to_owned).unwrap_or_default()
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

fn content_embedding(input: openai::Embedding) -> gemini::ContentEmbedding {
    gemini::ContentEmbedding {
        values: input.embedding.into_iter().map(f64_to_f32).collect(),
        shape: Vec::new(),
        extra: Default::default(),
    }
}

fn f64_to_f32(value: f64) -> f32 {
    value as f32
}

fn usage_metadata(
    input: openai::EmbeddingUsage,
) -> Result<gemini::EmbeddingUsageMetadata, TransformError> {
    Ok(gemini::EmbeddingUsageMetadata {
        total_token_count: Some(u32_to_i32(input.total_tokens)),
        input_token_count: Some(u32_to_i32(input.prompt_tokens)),
        prompt_tokens_details: Vec::new(),
        extra: Default::default(),
    })
}

trait EmbeddingInputExt {
    fn into_strings(self) -> Vec<String>;
}

impl EmbeddingInputExt for openai::EmbeddingInput {
    fn into_strings(self) -> Vec<String> {
        match self {
            Self::Text(text) => vec![text],
            Self::TextList(values) => values,
            Self::TokenList(_) => vec![String::new()],
            Self::TokenLists(values) => values.into_iter().map(|_| String::new()).collect(),
        }
    }
}
