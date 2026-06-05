use serde::{Deserialize, Serialize};

use super::JsonObject;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorResponse {
    #[serde(rename = "type")]
    pub type_: ErrorResponseType,
    pub error: ErrorBody,
    pub request_id: String,
    #[serde(default, flatten, skip_serializing_if = "JsonObject::is_empty")]
    pub extra: JsonObject,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErrorResponseType {
    #[serde(rename = "error")]
    Error,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ErrorBody {
    InvalidRequest(InvalidRequestError),
    Authentication(AuthenticationError),
    Billing(BillingError),
    Permission(PermissionError),
    NotFound(NotFoundError),
    RateLimit(RateLimitError),
    GatewayTimeout(GatewayTimeoutError),
    Api(ApiError),
    Overloaded(OverloadedError),
    Unknown(UnknownError),
}

macro_rules! api_error {
    ($name:ident, $type_name:ident, $variant:ident, $wire:literal) => {
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        pub struct $name {
            pub message: String,
            #[serde(rename = "type")]
            pub type_: $type_name,
            #[serde(default, flatten, skip_serializing_if = "JsonObject::is_empty")]
            pub extra: JsonObject,
        }

        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
        pub enum $type_name {
            #[serde(rename = $wire)]
            $variant,
        }
    };
}

api_error!(
    InvalidRequestError,
    InvalidRequestErrorType,
    InvalidRequestError,
    "invalid_request_error"
);

api_error!(
    AuthenticationError,
    AuthenticationErrorType,
    AuthenticationError,
    "authentication_error"
);

api_error!(
    BillingError,
    BillingErrorType,
    BillingError,
    "billing_error"
);

api_error!(
    PermissionError,
    PermissionErrorType,
    PermissionError,
    "permission_error"
);

api_error!(
    NotFoundError,
    NotFoundErrorType,
    NotFoundError,
    "not_found_error"
);

api_error!(
    RateLimitError,
    RateLimitErrorType,
    RateLimitError,
    "rate_limit_error"
);

api_error!(
    GatewayTimeoutError,
    GatewayTimeoutErrorType,
    GatewayTimeoutError,
    "timeout_error"
);

api_error!(ApiError, ApiErrorType, ApiError, "api_error");

api_error!(
    OverloadedError,
    OverloadedErrorType,
    OverloadedError,
    "overloaded_error"
);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnknownError {
    pub message: String,
    #[serde(rename = "type")]
    pub type_: String,
    #[serde(default, flatten, skip_serializing_if = "JsonObject::is_empty")]
    pub extra: JsonObject,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_error_response_deserializes_known_error() {
        let parsed: ErrorResponse = serde_json::from_value(serde_json::json!({
            "type": "error",
            "error": {
                "type": "api_error",
                "message": "Internal service failure"
            },
            "request_id": "req_123"
        }))
        .expect("known error response should deserialize");

        assert!(matches!(parsed.error, ErrorBody::Api(_)));
    }

    #[test]
    fn api_error_response_keeps_unknown_error_type_extensible() {
        let parsed: ErrorResponse = serde_json::from_value(serde_json::json!({
            "type": "error",
            "error": {
                "type": "future_error",
                "message": "A future error",
                "extra_field": true
            },
            "request_id": "req_456"
        }))
        .expect("unknown error response should deserialize");

        match parsed.error {
            ErrorBody::Unknown(error) => {
                assert_eq!(error.type_, "future_error");
                assert!(error.extra.contains_key("extra_field"));
            }
            other => panic!("expected unknown error, got {other:?}"),
        }
    }
}
