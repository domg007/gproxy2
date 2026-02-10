mod generate;
mod helpers;
mod ops;
mod stream;
mod types;
mod usage;

#[cfg(test)]
mod tests;

pub use types::{
    CountTokensRequest, CountTokensResponse, GenerateContentRequest, GenerateContentResponse,
    MemoryTraceSummarizeRequest, MemoryTraceSummarizeResponse, ModelGetRequest, ModelGetResponse,
    ModelListRequest, ModelListResponse, Op, Proto, Request, Response, ResponseCancelRequest,
    ResponseCancelResponse, ResponseCompactRequest, ResponseCompactResponse, ResponseDeleteRequest,
    ResponseDeleteResponse, ResponseGetRequest, ResponseGetResponse, ResponseListInputItemsRequest,
    ResponseListInputItemsResponse, StreamEvent, StreamFormat, TransformContext, TransformError,
    stream_format,
};

pub use ops::{transform_request, transform_response};
pub use stream::{NostreamToStream, StreamToNostream, StreamTransformer};
pub use usage::{
    CountTokensFn, OutputAccumulator, UsageAccumulator, UsageError, UsageSummary,
    fallback_usage_with_count_tokens, output_for_counting, usage_from_response,
};
