//! Gemini -> OpenAI embedding transforms.

use crate::protocol::{gemini, openai};
use crate::transform::{TransformContext, TransformError};

use super::{DEFAULT_OPENAI_EMBEDDING_MODEL, i32_to_u32};

pub fn request(
    input: gemini::BatchEmbedContentsRequest,
    ctx: &TransformContext,
) -> Result<openai::EmbeddingRequest, TransformError> {
    let requests = input.requests;
    let mut inputs = Vec::with_capacity(requests.len());
    let mut model = None;
    let mut dimensions = None;

    for request in requests {
        let converted = request_parts(request, ctx)?;
        inputs.push(converted.text);
        merge_model(&mut model, converted.model);
        merge_dimensions(&mut dimensions, converted.dimensions);
    }

    Ok(openai::EmbeddingRequest {
        input: strings_to_openai_input(inputs),
        model: model
            .unwrap_or_else(|| DEFAULT_OPENAI_EMBEDDING_MODEL.to_owned())
            .into(),
        dimensions,
        encoding_format: Some(openai::EmbeddingEncodingFormat::Float),
        user: None,
        extra: Default::default(),
    })
}

pub fn single_request(
    input: gemini::EmbedContentRequest,
    ctx: &TransformContext,
) -> Result<openai::EmbeddingRequest, TransformError> {
    let converted = request_parts(input, ctx)?;
    Ok(openai::EmbeddingRequest {
        input: openai::EmbeddingInput::Text(converted.text),
        model: converted
            .model
            .unwrap_or_else(|| DEFAULT_OPENAI_EMBEDDING_MODEL.to_owned())
            .into(),
        dimensions: converted.dimensions,
        encoding_format: Some(openai::EmbeddingEncodingFormat::Float),
        user: None,
        extra: Default::default(),
    })
}

pub fn response(
    input: gemini::BatchEmbedContentsResponse,
    _: &TransformContext,
) -> Result<openai::EmbeddingResponse, TransformError> {
    Ok(openai::EmbeddingResponse {
        data: input
            .embeddings
            .into_iter()
            .enumerate()
            .map(|(index, embedding)| openai_embedding(embedding, index))
            .collect(),
        model: DEFAULT_OPENAI_EMBEDDING_MODEL.to_owned().into(),
        object: openai::ListObjectType::List,
        usage: usage(input.usage_metadata)?,
        extra: Default::default(),
    })
}

pub fn single_response(
    input: gemini::EmbedContentResponse,
    ctx: &TransformContext,
) -> Result<openai::EmbeddingResponse, TransformError> {
    response(
        gemini::BatchEmbedContentsResponse {
            embeddings: input.embedding.into_iter().collect(),
            usage_metadata: input.usage_metadata,
            extra: Default::default(),
        },
        ctx,
    )
}

struct ConvertedRequest {
    text: String,
    model: Option<String>,
    dimensions: Option<u32>,
}

fn request_parts(
    input: gemini::EmbedContentRequest,
    _: &TransformContext,
) -> Result<ConvertedRequest, TransformError> {
    Ok(ConvertedRequest {
        text: content_text(input.content),
        model: input.model,
        dimensions: input.output_dimensionality.map(i32_to_u32),
    })
}

fn content_text(input: Option<gemini::Content>) -> String {
    let Some(content) = input else {
        return String::new();
    };
    let mut text = String::new();

    for part in content.parts {
        if let Some(gemini::PartData::Text { text: value }) = part.data {
            text.push_str(&value);
        }
    }

    text
}

fn strings_to_openai_input(values: Vec<String>) -> openai::EmbeddingInput {
    let mut values = values;
    if values.len() == 1 {
        openai::EmbeddingInput::Text(values.remove(0))
    } else {
        openai::EmbeddingInput::TextList(values)
    }
}

fn merge_model(target: &mut Option<String>, next: Option<String>) {
    let Some(next) = next else {
        return;
    };
    if target.is_none() {
        *target = Some(next);
    }
}

fn merge_dimensions(target: &mut Option<u32>, next: Option<u32>) {
    let Some(next) = next else {
        return;
    };
    if target.is_none() {
        *target = Some(next);
    }
}

fn openai_embedding(input: gemini::ContentEmbedding, index: usize) -> openai::Embedding {
    openai::Embedding {
        embedding: input.values.into_iter().map(f64::from).collect(),
        index: u32::try_from(index).unwrap_or(u32::MAX),
        object: openai::EmbeddingObjectType::Embedding,
        extra: Default::default(),
    }
}

fn usage(
    input: Option<gemini::EmbeddingUsageMetadata>,
) -> Result<openai::EmbeddingUsage, TransformError> {
    let Some(input) = input else {
        return Ok(openai::EmbeddingUsage {
            prompt_tokens: 0,
            total_tokens: 0,
            extra: Default::default(),
        });
    };

    Ok(openai::EmbeddingUsage {
        prompt_tokens: input.input_token_count.map(i32_to_u32).unwrap_or_default(),
        total_tokens: input.total_token_count.map(i32_to_u32).unwrap_or_default(),
        extra: Default::default(),
    })
}
