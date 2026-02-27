use reqwest::Client;
use serde::Deserialize;
use tracing::error;

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
    // Banco Popular's site is behind Incapsula WAF which blocks non-browser requests.
    // We attempt to scrape anyway — it may work from certain server IPs.
    // The rate values are in input fields: compra_peso_dolar_modal (buy) and venta_peso_dolar_modal (sell).
    let response = client
        .get("https://popularenlinea.com/personas/Paginas/Home.aspx")
        .header("User-Agent", USER_AGENT)
        .header("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")
        .header("Accept-Language", "es-DO,es;q=0.9,en;q=0.8")
        .send()
        .await
        .map_err(|e| error!("Popular request failed: {e}"))
        .ok()?;

    if !response.status().is_success() {
        error!("Popular returned HTTP {}", response.status());
        return None;
    }

    let html = response
        .text()
        .await
        .map_err(|e| error!("Popular body read failed: {e}"))
        .ok()?;

    // Try to extract buy rate from compra_peso_dolar_modal input value
    let buy_re = regex::Regex::new(
        r#"id="compra_peso_dolar_modal"[^>]*value="(\d+\.?\d*)""#
    ).ok()?;
    // Try to extract sell rate from venta_peso_dolar_modal input value
    let sell_re = regex::Regex::new(
        r#"id="venta_peso_dolar_modal"[^>]*value="(\d+\.?\d*)""#
    ).ok()?;

    let buy = buy_re.captures(&html);
    let sell = sell_re.captures(&html);

    if let (Some(buy_caps), Some(sell_caps)) = (buy, sell) {
        let buy_rate: f64 = buy_caps[1].parse().ok()?;
        let sell_rate: f64 = sell_caps[1].parse().ok()?;
        return Some(FetchedRate {
            bank_name: "Banco Popular".into(),
            bank_class: "popular".into(),
            dollar_buy_rate: buy_rate,
            dollar_sell_rate: sell_rate,
        });
    }

    // Fallback: try to find rates set via JavaScript (e.g. .val('60.10') or = '60.10')
    let js_buy_re = regex::Regex::new(
        r#"compra_peso_dolar_modal[^)]*?(\d{2,3}\.\d{2})"#
    ).ok()?;
    let js_sell_re = regex::Regex::new(
        r#"venta_peso_dolar_modal[^)]*?(\d{2,3}\.\d{2})"#
    ).ok()?;

    if let (Some(buy_caps), Some(sell_caps)) = (js_buy_re.captures(&html), js_sell_re.captures(&html)) {
        let buy_rate: f64 = buy_caps[1].parse().ok()?;
        let sell_rate: f64 = sell_caps[1].parse().ok()?;
        return Some(FetchedRate {
            bank_name: "Banco Popular".into(),
            bank_class: "popular".into(),
            dollar_buy_rate: buy_rate,
            dollar_sell_rate: sell_rate,
        });
    }

    error!("Popular: could not parse rates from HTML");
    None
}
