use tiktoken_rs::{CoreBPE, get_bpe_from_model, o200k_base, o200k_harmony};

pub fn is_gpt_like_model(model: &str) -> bool {
    let model = model.to_ascii_lowercase();
    model.starts_with("gpt")
        || model.starts_with("chatgpt")
        || model.starts_with("o1")
        || model.starts_with("o3")
        || model.starts_with("o4")
        || model.starts_with("ft:gpt")
        || model.contains("gpt-")
}

pub fn count_tiktoken_tokens(model: &str, text: &str) -> Result<usize, String> {
    let bpe = build_bpe(model)?;
    let count = bpe.encode_ordinary(text).len();
    Ok(count)
}

fn build_bpe(model: &str) -> Result<CoreBPE, String> {
    if let Ok(bpe) = get_bpe_from_model(model) {
        return Ok(bpe);
    }
    if let Ok(bpe) = o200k_base() {
        return Ok(bpe);
    }
    o200k_harmony().map_err(|err| err.to_string())
}
