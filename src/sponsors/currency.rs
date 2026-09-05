use serde::Deserialize;
use std::sync::LazyLock;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExchangeRate {
    value: f64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExchangeRateData {
    currency_exchange_rate: Vec<ExchangeRate>,
}

#[derive(Deserialize)]
struct ExchangeRateResponse {
    data: Option<ExchangeRateData>,
    errors: Option<Vec<serde_json::Value>>,
}

static RATE_CACHE: LazyLock<moka::future::Cache<(String, String), f64>> = LazyLock::new(|| {
    moka::future::Cache::builder()
        .time_to_live(std::time::Duration::from_hours(6))
        .max_capacity(64)
        .build()
});

pub fn amount(cents: i64, currency: &str) -> String {
    let value = cents as f64 / 100.0;

    match currency {
        "USD" => format!("${value:.2}"),
        "EUR" => format!("€{value:.2}"),
        "GBP" => format!("£{value:.2}"),
        "JPY" => format!("¥{value:.2}"),
        _ => format!("{value:.2} {currency}"),
    }
}

async fn fetch_rate(env: &crate::env::Env, from: &str, to: &str) -> Result<f64, anyhow::Error> {
    let query = format!(
        r#"
        query {{
            currencyExchangeRate(requests: [{{fromCurrency: {from}, toCurrency: {to}}}]) {{
                value
            }}
        }}
        "#
    );

    let mut request = super::CLIENT
        .post(super::opencollective::GRAPHQL_URL)
        .json(&serde_json::json!({ "query": query }));

    // Rates are quoted by Open Collective, so without the token these spend the same
    // heavily limited anonymous budget the ledger pages need.
    if let Some(token) = &env.opencollective_token {
        request = request.header("Personal-Token", token);
    }

    let body = request.send().await?.error_for_status()?.text().await?;

    let response: ExchangeRateResponse = serde_json::from_str(&body).map_err(|err| {
        anyhow::anyhow!("failed to decode exchange rate response: {err} ({body})")
    })?;

    if let Some(errors) = response.errors.filter(|errors| !errors.is_empty()) {
        return Err(anyhow::anyhow!("exchange rate errors: {:?}", errors));
    }

    response
        .data
        .and_then(|data| data.currency_exchange_rate.into_iter().next())
        .map(|rate| rate.value)
        .ok_or_else(|| anyhow::anyhow!("no exchange rate returned for {from} -> {to}"))
}

pub async fn rate(env: &crate::env::Env, from: &str, to: &str) -> Result<f64, anyhow::Error> {
    if from == to {
        return Ok(1.0);
    }

    RATE_CACHE
        .try_get_with(
            (from.to_string(), to.to_string()),
            fetch_rate(env, from, to),
        )
        .await
        .map_err(|err| anyhow::anyhow!("{err}"))
}

pub async fn convert(
    env: &crate::env::Env,
    cents: i64,
    from: &str,
    to: &str,
) -> Result<i64, anyhow::Error> {
    if from == to || cents == 0 {
        return Ok(cents);
    }

    Ok((cents as f64 * rate(env, from, to).await?).round() as i64)
}
