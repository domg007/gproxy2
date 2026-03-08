use base64::Engine as _;
use serde_json::{Value, json};

#[derive(Debug, Clone, PartialEq, Eq)]
struct GrokUploadFileRequest {
    file_name: String,
    file_mime_type: String,
    content: String,
}

pub(super) fn validate_data_uri_reference(value: &str) -> Result<(), String> {
    parse_data_uri_upload(value).map(|_| ())
}

pub(super) fn build_grok_upload_file_body(value: &str) -> Result<Vec<u8>, String> {
    let upload = parse_data_uri_upload(value)?;
    serde_json::to_vec(&json!({
        "fileName": upload.file_name,
        "fileMimeType": upload.file_mime_type,
        "content": upload.content,
    }))
    .map_err(|err| err.to_string())
}

pub(super) async fn extract_uploaded_asset_url(response: wreq::Response) -> Result<String, String> {
    let body = response.text().await.map_err(|err| err.to_string())?;
    let payload = serde_json::from_str::<Value>(body.as_str()).map_err(|err| err.to_string())?;
    let file_uri = payload
        .get("fileUri")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "grok upload-file response returned no fileUri".to_string())?;
    Ok(asset_url_from_file_uri(file_uri))
}

fn parse_data_uri_upload(value: &str) -> Result<GrokUploadFileRequest, String> {
    let value = value.trim();
    let Some(rest) = value.strip_prefix("data:") else {
        return Err("invalid data URI: missing data: prefix".to_string());
    };
    let Some((header, body)) = rest.split_once(',') else {
        return Err("invalid data URI: missing content separator".to_string());
    };
    if !header
        .split(';')
        .any(|segment| segment.eq_ignore_ascii_case("base64"))
    {
        return Err("invalid data URI: missing base64 marker".to_string());
    }
    let mime = header
        .split(';')
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("application/octet-stream")
        .to_ascii_lowercase();
    let content = body
        .chars()
        .filter(|ch| !ch.is_ascii_whitespace())
        .collect::<String>();
    if content.is_empty() {
        return Err("invalid data URI: empty content".to_string());
    }
    base64::engine::general_purpose::STANDARD
        .decode(content.as_bytes())
        .map_err(|err| format!("invalid data URI base64 content: {err}"))?;

    Ok(GrokUploadFileRequest {
        file_name: format!("upload.{}", extension_from_mime(mime.as_str())),
        file_mime_type: mime,
        content,
    })
}

fn asset_url_from_file_uri(file_uri: &str) -> String {
    let file_uri = file_uri.trim();
    if file_uri.starts_with("http://") || file_uri.starts_with("https://") {
        return file_uri.to_string();
    }
    format!(
        "https://assets.grok.com/{}",
        file_uri.trim_start_matches('/')
    )
}

fn extension_from_mime(mime: &str) -> String {
    let subtype = mime
        .split('/')
        .nth(1)
        .unwrap_or("bin")
        .split(';')
        .next()
        .unwrap_or("bin")
        .split('+')
        .next()
        .unwrap_or("bin");
    let normalized = subtype
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();
    if normalized.is_empty() {
        "bin".to_string()
    } else {
        normalized
    }
}

#[cfg(test)]
mod tests {
    use super::{
        asset_url_from_file_uri, build_grok_upload_file_body, validate_data_uri_reference,
    };

    #[test]
    fn validate_data_uri_accepts_base64_payload() {
        validate_data_uri_reference("data:image/png;base64,aGVsbG8=").unwrap();
    }

    #[test]
    fn validate_data_uri_rejects_non_base64_payload() {
        let err = validate_data_uri_reference("data:image/png,hello").unwrap_err();
        assert!(err.contains("base64"));
    }

    #[test]
    fn build_upload_body_uses_expected_shape() {
        let body = build_grok_upload_file_body("data:image/png;base64,aGVsbG8=").unwrap();
        let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(payload["fileName"], "upload.png");
        assert_eq!(payload["fileMimeType"], "image/png");
        assert_eq!(payload["content"], "aGVsbG8=");
    }

    #[test]
    fn asset_url_normalizes_relative_file_uri() {
        assert_eq!(
            asset_url_from_file_uri("/foo/bar.png"),
            "https://assets.grok.com/foo/bar.png"
        );
    }
}
