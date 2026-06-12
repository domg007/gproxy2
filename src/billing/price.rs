//! §17 pricing: per-million-token Decimal rates from `provider_models.pricing_json`.

use rust_decimal::Decimal;
use serde_json::Value;

use crate::usage::NormalizedUsage;

/// Per-million-token rates. Missing/unconfigured keys are zero-priced —
/// tokens are still recorded, they just cost nothing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Pricing {
    pub input: Decimal,
    pub output: Decimal,
    pub cache_read: Decimal,
    pub cache_creation: Decimal,
    /// Flat price PER IMAGE (not per-million) — image generation is billed by
    /// count, not tokens. Zero when unconfigured.
    pub image: Decimal,
}

/// Parse [`Pricing`] from a `pricing_json` value. Rates are Decimal-as-text
/// (the records money convention); plain JSON numbers are accepted too.
/// Missing or malformed keys fall back to 0 (malformed warns).
pub fn pricing_from(pricing_json: Option<&Value>) -> Pricing {
    let Some(v) = pricing_json else {
        return Pricing::default();
    };
    Pricing {
        input: rate(v, "input"),
        output: rate(v, "output"),
        cache_read: rate(v, "cache_read"),
        cache_creation: rate(v, "cache_creation"),
        image: rate(v, "image"),
    }
}

fn rate(v: &Value, key: &str) -> Decimal {
    let parsed = match v.get(key) {
        None | Some(Value::Null) => return Decimal::ZERO,
        Some(Value::String(s)) => s.parse::<Decimal>(),
        Some(Value::Number(n)) => n.to_string().parse::<Decimal>(),
        Some(_) => {
            tracing::warn!(key, "pricing_json: non-numeric rate; treating as 0");
            return Decimal::ZERO;
        }
    };
    parsed.unwrap_or_else(|_| {
        tracing::warn!(key, "pricing_json: unparsable rate; treating as 0");
        Decimal::ZERO
    })
}

/// Cost of `u` at rates `p`: Σ tokens × rate / 1_000_000 (exact Decimal math).
pub fn cost(u: &NormalizedUsage, p: &Pricing) -> Decimal {
    let million = Decimal::from(1_000_000u64);
    (Decimal::from(u.input) * p.input
        + Decimal::from(u.output) * p.output
        + Decimal::from(u.cache_read) * p.cache_read
        + Decimal::from(u.cache_creation) * p.cache_creation)
        / million
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn pricing_parse_and_exact_cost_math() {
        // Text rate, number rate, missing keys → 0.
        let p = pricing_from(Some(&json!({"input": "3.00", "output": 15})));
        assert_eq!(p.input, Decimal::from(3));
        assert_eq!(p.output, Decimal::from(15));
        assert_eq!(p.cache_read, Decimal::ZERO);
        assert_eq!(p.cache_creation, Decimal::ZERO);
        assert_eq!(pricing_from(None), Pricing::default());

        // Per-image flat rate (image generation is billed by count, not tokens).
        let img = pricing_from(Some(&json!({"image": "0.04"})));
        assert_eq!(img.image, "0.04".parse::<Decimal>().unwrap());
        assert_eq!(
            Decimal::from(3u64) * img.image,
            "0.12".parse::<Decimal>().unwrap()
        );

        // 1500 input @ 3.00/M = 0.0045; cache tokens are free here but counted.
        let u = NormalizedUsage {
            input: 1500,
            output: 2000,
            cache_read: 10_000,
            cache_creation: 500,
            reasoning: 0,
        };
        let expected: Decimal = "0.0345".parse().unwrap(); // 0.0045 + 2000*15/1e6
        assert_eq!(cost(&u, &p), expected);
        assert_eq!(
            cost(
                &NormalizedUsage {
                    input: 1500,
                    ..Default::default()
                },
                &p
            ),
            "0.0045".parse().unwrap()
        );
    }
}
