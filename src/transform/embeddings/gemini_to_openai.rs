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
        merge_model(&mut model, converted.model)?;
        merge_dimensions(&mut dimensions, converted.dimensions)?;
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
            .collect::<Result<Vec<_>, _>>()?,
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
    let embedding = input.embedding.ok_or(TransformError::InvalidInput {
        reason: "Gemini embedding response is missing `embedding`".to_owned(),
    })?;

    response(
        gemini::BatchEmbedContentsResponse {
            embeddings: vec![embedding],
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
    if input.task_type.is_some() {
        return Err(TransformError::UnsupportedField {
            field: "task_type",
            reason: "OpenAI embedding request has no task type field",
        });
    }
    if input.title.is_some() {
        return Err(TransformError::UnsupportedField {
            field: "title",
            reason: "OpenAI embedding request has no document title field",
        });
    }

    Ok(ConvertedRequest {
        text: content_text(input.content)?,
        model: input.model,
        dimensions: input
            .output_dimensionality
            .map(|value| i32_to_u32(value, "output_dimensionality"))
            .transpose()?,
    })
}

fn content_text(input: Option<gemini::Content>) -> Result<String, TransformError> {
    let content = input.ok_or(TransformError::InvalidInput {
        reason: "Gemini embedding request is missing `content`".to_owned(),
    })?;
    let mut text = String::new();

    for part in content.parts {
        if part.thought.is_some()
            || part.thought_signature.is_some()
            || part.part_metadata.is_some()
            || part.media_resolution.is_some()
            || part.metadata.is_some()
        {
            return Err(TransformError::UnsupportedField {
                field: "content.parts",
                reason: "OpenAI embedding input cannot represent Gemini part metadata",
            });
        }

        match part.data {
            Some(gemini::PartData::Text { text: value }) => text.push_str(&value),
            Some(_) => {
                return Err(TransformError::UnsupportedField {
                    field: "content.parts",
                    reason: "OpenAI embedding input supports text only",
                });
            }
            None => {}
        }
    }

    Ok(text)
}

fn strings_to_openai_input(values: Vec<String>) -> openai::EmbeddingInput {
    let mut values = values;
    if values.len() == 1 {
        openai::EmbeddingInput::Text(values.remove(0))
    } else {
        openai::EmbeddingInput::TextList(values)
    }
}

fn merge_model(target: &mut Option<String>, next: Option<String>) -> Result<(), TransformError> {
    let Some(next) = next else {
        return Ok(());
    };
    match target {
        Some(current) if current != &next => Err(TransformError::InvalidInput {
            reason: "Gemini batch embedding requests use different models".to_owned(),
        }),
        Some(_) => Ok(()),
        None => {
            *target = Some(next);
            Ok(())
        }
    }
}

fn merge_dimensions(target: &mut Option<u32>, next: Option<u32>) -> Result<(), TransformError> {
    let Some(next) = next else {
        return Ok(());
    };
    match target {
        Some(current) if *current != next => Err(TransformError::InvalidInput {
            reason: "Gemini batch embedding requests use different output dimensionalities"
                .to_owned(),
        }),
        Some(_) => Ok(()),
        None => {
            *target = Some(next);
            Ok(())
        }
    }
}

fn openai_embedding(
    input: gemini::ContentEmbedding,
    index: usize,
) -> Result<openai::Embedding, TransformError> {
    Ok(openai::Embedding {
        embedding: input.values.into_iter().map(f64::from).collect(),
        index: u32::try_from(index).map_err(|_| TransformError::LossyField {
            field: "index",
            reason: "embedding index cannot be represented as u32",
        })?,
        object: openai::EmbeddingObjectType::Embedding,
        extra: Default::default(),
    })
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
        prompt_tokens: input
            .input_token_count
            .map(|value| i32_to_u32(value, "usage_metadata.input_token_count"))
            .transpose()?
            .unwrap_or_default(),
        total_tokens: input
            .total_token_count
            .map(|value| i32_to_u32(value, "usage_metadata.total_token_count"))
            .transpose()?
            .unwrap_or_default(),
        extra: Default::default(),
    })
}
