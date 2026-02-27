use reqwest::Client;
use serde::Deserialize;
use std::env;
use std::sync::Arc;
use tracing::{error, info, warn};

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

fn browser_headers(rb: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
    rb.header("User-Agent", "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/145.0.0.0 Safari/537.36")
        .header("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,*/*;q=0.8")
        .header("Accept-Language", "en-US,en;q=0.9,es-US;q=0.8,es;q=0.7")
        .header("Sec-Ch-Ua", r#""Not:A-Brand";v="99", "Google Chrome";v="145", "Chromium";v="145""#)
        .header("Sec-Ch-Ua-Mobile", "?0")
        .header("Sec-Ch-Ua-Platform", r#""macOS""#)
        .header("Sec-Fetch-Dest", "document")
        .header("Sec-Fetch-Mode", "navigate")
        .header("Sec-Fetch-Site", "none")
        .header("Sec-Fetch-User", "?1")
        .header("Upgrade-Insecure-Requests", "1")
}

pub async fn fetch_popular(_client: &Client) -> Option<FetchedRate> {
    // Banco Popular exposes rates via SharePoint REST API (XML/OData).
    // Site is behind Incapsula WAF — we emulate a browser session:
    // 1. Visit the homepage to collect Incapsula cookies
    // 2. Fetch the Incapsula challenge script to get session cookies
    // 3. Use accumulated cookies to call the API

    // If POPULAR_PROXY_URL is set, use it directly (skip browser emulation)
    if let Ok(proxy_url) = env::var("POPULAR_PROXY_URL") {
        info!("Popular: using proxy URL");
        let response = _client
            .get(&proxy_url)
            .header("User-Agent", USER_AGENT)
            .header("Accept", "application/xml")
            .send()
            .await
            .map_err(|e| error!("Popular proxy request failed: {e}"))
            .ok()?;
        if response.status().is_success() {
            let xml = response.text().await.ok()?;
            return parse_popular_xml(&xml);
        }
        warn!("Popular proxy returned non-200, falling back to direct");
    }

    // Build a client with a cookie jar to accumulate Incapsula cookies
    let jar = Arc::new(reqwest::cookie::Jar::default());
    let popular_client = reqwest::Client::builder()
        .cookie_provider(jar.clone())
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .ok()?;

    // Step 1: Hit the homepage to trigger Incapsula challenge and collect initial cookies
    info!("Popular: step 1 - visiting homepage for cookies");
    let homepage = browser_headers(popular_client.get("https://popularenlinea.com/personas/Paginas/Home.aspx"))
        .send()
        .await
        .map_err(|e| error!("Popular homepage request failed: {e}"))
        .ok()?;

    let body = homepage.text().await.unwrap_or_default();

    // Step 2: Extract and fetch the Incapsula challenge script (sets session cookies)
    let script_re = regex::Regex::new(r#"src="(/_Incapsula_Resource\?SWJIYLWA=[^"]+)""#).ok()?;
    if let Some(caps) = script_re.captures(&body) {
        let script_url = format!("https://popularenlinea.com{}", &caps[1]);
        info!("Popular: step 2 - fetching Incapsula challenge script");
        let _ = browser_headers(popular_client.get(&script_url))
            .header("Referer", "https://popularenlinea.com/personas/Paginas/Home.aspx")
            .send()
            .await;
    }

    // Small delay to mimic browser behavior
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // Step 3: Retry the homepage with accumulated cookies
    info!("Popular: step 3 - retrying homepage with cookies");
    let _ = browser_headers(popular_client.get("https://popularenlinea.com/personas/Paginas/Home.aspx"))
        .send()
        .await;

    tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;

    // Step 4: Now fetch the actual rates API with the session cookies
    info!("Popular: step 4 - fetching rates API");
    let api_url = "https://popularenlinea.com/_api/web/lists/getbytitle('Rates')/items?$filter=ItemID%20eq%20%271%27";
    let response = browser_headers(popular_client.get(api_url))
        .header("Accept", "application/xml")
        .send()
        .await
        .map_err(|e| error!("Popular API request failed: {e}"))
        .ok()?;

    if !response.status().is_success() {
        error!("Popular API returned HTTP {}", response.status());
        return None;
    }

    let xml = response
        .text()
        .await
        .map_err(|e| error!("Popular body read failed: {e}"))
        .ok()?;

    parse_popular_xml(&xml)
}

fn parse_popular_xml(xml: &str) -> Option<FetchedRate> {
    let buy_re = regex::Regex::new(r"<d:DollarBuyRate[^>]*>(\d+\.?\d*)</d:DollarBuyRate>").ok()?;
    let sell_re = regex::Regex::new(r"<d:DollarSellRate[^>]*>(\d+\.?\d*)</d:DollarSellRate>").ok()?;

    let buy_rate: f64 = buy_re
        .captures(xml)
        .or_else(|| { error!("Popular: DollarBuyRate not found in XML"); None })?[1]
        .parse()
        .ok()?;

    let sell_rate: f64 = sell_re
        .captures(xml)
        .or_else(|| { error!("Popular: DollarSellRate not found in XML"); None })?[1]
        .parse()
        .ok()?;

    info!("Popular: buy={buy_rate:.2} sell={sell_rate:.2}");

    Some(FetchedRate {
        bank_name: "Banco Popular".into(),
        bank_class: "popular".into(),
        dollar_buy_rate: buy_rate,
        dollar_sell_rate: sell_rate,
    })
}
