use reqwest::Client;
use serde::Deserialize;
use std::env;
use tracing::{error, info};

const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.124 Safari/537.36";

pub struct FetchedRate {
    pub bank_name: String,
    pub bank_class: String,
    pub dollar_buy_rate: f64,
    pub dollar_sell_rate: f64,
}

// --- BHD API response types ---

#[derive(Debug, Deserialize)]
struct BhdApiResponse {
    data: BhdData,
}

#[derive(Debug, Deserialize)]
struct BhdData {
    attributes: BhdAttributes,
}

#[derive(Debug, Deserialize)]
struct BhdAttributes {
    #[serde(rename = "exchangeRates")]
    exchange_rates: Vec<BhdExchangeRate>,
}

#[derive(Debug, Deserialize)]
struct BhdExchangeRate {
    currency: String,
    #[serde(rename = "buyingRate")]
    buying_rate: f64,
    #[serde(rename = "sellingRate")]
    selling_rate: f64,
}

// --- Fetchers ---

pub async fn fetch_banreservas(client: &Client) -> Option<FetchedRate> {
    let response = client
        .get("https://www.banreservas.com/calculadoras/")
        .header("User-Agent", USER_AGENT)
        .send()
        .await
        .map_err(|e| error!("Banreservas request failed: {e}"))
        .ok()?;

    let html = response
        .text()
        .await
        .map_err(|e| error!("Banreservas body read failed: {e}"))
        .ok()?;

    let re = regex::Regex::new(r"(?s)Compra\s*(\d+\.\d+).*?Venta\s*(\d+\.\d+)").ok()?;
    let caps = re.captures(&html).or_else(|| {
        error!("Banreservas: could not parse rates from HTML");
        None
    })?;

    Some(FetchedRate {
        bank_name: "Banreservas".into(),
        bank_class: "banreservas".into(),
        dollar_buy_rate: caps[1].parse().ok()?,
        dollar_sell_rate: caps[2].parse().ok()?,
    })
}

pub async fn fetch_bhd(client: &Client) -> Option<FetchedRate> {
    let response = client
        .get("https://backend.bhd.com.do/api/modal-cambio-rate?populate=deep")
        .header("User-Agent", USER_AGENT)
        .send()
        .await
        .map_err(|e| error!("BHD request failed: {e}"))
        .ok()?;

    let data: BhdApiResponse = response
        .json()
        .await
        .map_err(|e| error!("BHD JSON parse failed: {e}"))
        .ok()?;

    let usd = data
        .data
        .attributes
        .exchange_rates
        .iter()
        .find(|r| r.currency == "USD")
        .or_else(|| {
            error!("BHD: USD rate not found");
            None
        })?;

    Some(FetchedRate {
        bank_name: "BHD".into(),
        bank_class: "bhd".into(),
        dollar_buy_rate: usd.buying_rate,
        dollar_sell_rate: usd.selling_rate,
    })
}

pub async fn fetch_popular(client: &Client) -> Option<FetchedRate> {
    // Banco Popular exposes rates via SharePoint REST API (XML/OData).
    // Fields: d:DollarBuyRate, d:DollarSellRate
    // Site is behind Incapsula WAF — use POPULAR_PROXY_URL to route through a proxy if blocked.
    let default_url = "https://popularenlinea.com/_api/web/lists/getbytitle('Rates')/items?$filter=ItemID%20eq%20%271%27";
    let url = env::var("POPULAR_PROXY_URL").unwrap_or_else(|_| default_url.to_string());
    info!("Popular: fetching from {url}");

    let response = client
        .get(&url)
        .header("User-Agent", USER_AGENT)
        .header("Accept", "application/xml")
        .send()
        .await
        .map_err(|e| error!("Popular request failed: {e}"))
        .ok()?;

    if !response.status().is_success() {
        error!("Popular returned HTTP {}", response.status());
        return None;
    }

    let xml = response
        .text()
        .await
        .map_err(|e| error!("Popular body read failed: {e}"))
        .ok()?;

    let buy_re = regex::Regex::new(r"<d:DollarBuyRate[^>]*>(\d+\.?\d*)</d:DollarBuyRate>").ok()?;
    let sell_re = regex::Regex::new(r"<d:DollarSellRate[^>]*>(\d+\.?\d*)</d:DollarSellRate>").ok()?;

    let buy_rate: f64 = buy_re
        .captures(&xml)
        .or_else(|| { error!("Popular: DollarBuyRate not found in XML"); None })?[1]
        .parse()
        .ok()?;

    let sell_rate: f64 = sell_re
        .captures(&xml)
        .or_else(|| { error!("Popular: DollarSellRate not found in XML"); None })?[1]
        .parse()
        .ok()?;

    Some(FetchedRate {
        bank_name: "Banco Popular".into(),
        bank_class: "popular".into(),
        dollar_buy_rate: buy_rate,
        dollar_sell_rate: sell_rate,
    })
}
