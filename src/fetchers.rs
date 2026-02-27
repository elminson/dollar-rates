use reqwest::Client;
use serde::Deserialize;
use std::env;
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

pub async fn fetch_popular(client: &Client) -> Option<FetchedRate> {
    // Banco Popular exposes rates via SharePoint REST API (XML/OData).
    // Site is behind Incapsula WAF which requires JS execution.
    // We use headless Chrome to pass the challenge, then extract the XML.

    // Fast path: if POPULAR_PROXY_URL is set, use it directly (no Chrome needed)
    if let Ok(proxy_url) = env::var("POPULAR_PROXY_URL") {
        info!("Popular: using proxy URL");
        let response = client
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
        warn!("Popular proxy returned non-200, falling back to headless Chrome");
    }

    // Use headless Chrome to bypass Incapsula WAF
    info!("Popular: launching headless Chrome");
    let result = tokio::task::spawn_blocking(|| -> Option<String> {
        use headless_chrome::{Browser, LaunchOptions};

        let chromium_path = env::var("CHROMIUM_PATH").unwrap_or_else(|_| "/usr/bin/chromium".into());
        info!("Popular: using Chromium at {chromium_path}");

        let launch_options = LaunchOptions {
            path: Some(std::path::PathBuf::from(&chromium_path)),
            sandbox: false,
            args: vec![
                std::ffi::OsStr::new("--no-sandbox"),
                std::ffi::OsStr::new("--disable-gpu"),
                std::ffi::OsStr::new("--disable-dev-shm-usage"),
            ],
            ..LaunchOptions::default()
        };

        let browser = Browser::new(launch_options)
            .map_err(|e| error!("Popular: Chrome launch failed: {e}"))
            .ok()?;

        let tab = browser.new_tab()
            .map_err(|e| error!("Popular: new tab failed: {e}"))
            .ok()?;

        // Step 1: Navigate to homepage, let Incapsula JS execute
        info!("Popular: navigating to homepage for Incapsula challenge");
        tab.navigate_to("https://popularenlinea.com/personas/Paginas/Home.aspx")
            .map_err(|e| error!("Popular: homepage navigation failed: {e}"))
            .ok()?;

        // Wait for Incapsula JS challenge to complete (~5 seconds)
        std::thread::sleep(std::time::Duration::from_secs(5));

        // Step 2: Navigate to the rates API with accumulated cookies
        info!("Popular: navigating to rates API");
        let api_url = "https://popularenlinea.com/_api/web/lists/getbytitle('Rates')/items?$filter=ItemID%20eq%20%271%27";
        tab.navigate_to(api_url)
            .map_err(|e| error!("Popular: API navigation failed: {e}"))
            .ok()?;

        // Wait for the API response to load
        std::thread::sleep(std::time::Duration::from_secs(3));

        // Step 3: Extract page content (the XML response)
        let content = tab.get_content()
            .map_err(|e| error!("Popular: get_content failed: {e}"))
            .ok()?;

        info!("Popular: got content, length={}", content.len());
        Some(content)
    })
    .await
    .map_err(|e| error!("Popular: spawn_blocking join failed: {e}"))
    .ok()??;

    // The browser may wrap the XML in HTML tags, extract the raw XML if needed
    let xml = if result.contains("<d:DollarBuyRate") {
        result
    } else {
        // Chrome wraps raw XML in an HTML page; extract the text content
        // The XML content is usually inside <pre> or as text nodes
        warn!("Popular: response may be wrapped in HTML, attempting extraction");
        result
    };

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
