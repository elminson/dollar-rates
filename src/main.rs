mod fetchers;

use chrono::{DateTime, Utc};
use reqwest::Client;
use rocket::serde::json::Json;
use rocket::{get, routes, State};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tokio::time::{interval, Duration};
use tracing::{error, info};

// --- Models ---

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct BankRate {
    pub id: i32,
    pub bank_name: String,
    pub bank_class: String,
    pub dollar_buy_rate: f64,
    pub dollar_sell_rate: f64,
    pub updated_at: Option<DateTime<Utc>>,
    pub created_at: Option<DateTime<Utc>>,
}

// --- Routes ---

#[get("/")]
fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "service": "dollar-rates",
    }))
}

#[get("/rates")]
async fn get_rates(pool: &State<PgPool>) -> Json<serde_json::Value> {
    match sqlx::query_as::<_, BankRate>("SELECT * FROM bank_rates ORDER BY bank_class")
        .fetch_all(pool.inner())
        .await
    {
        Ok(rates) => Json(serde_json::json!({
            "success": true,
            "data": rates,
        })),
        Err(e) => Json(serde_json::json!({
            "success": false,
            "error": e.to_string(),
        })),
    }
}

#[get("/rates/<bank_class>")]
async fn get_rate_by_bank(pool: &State<PgPool>, bank_class: &str) -> Json<serde_json::Value> {
    match sqlx::query_as::<_, BankRate>("SELECT * FROM bank_rates WHERE bank_class = $1")
        .bind(bank_class)
        .fetch_optional(pool.inner())
        .await
    {
        Ok(Some(rate)) => Json(serde_json::json!({
            "success": true,
            "data": rate,
        })),
        Ok(None) => Json(serde_json::json!({
            "success": false,
            "message": format!("Bank '{}' not found", bank_class),
        })),
        Err(e) => Json(serde_json::json!({
            "success": false,
            "error": e.to_string(),
        })),
    }
}

// --- Database operations ---

async fn upsert_rate(pool: &PgPool, rate: &fetchers::FetchedRate) {
    let result = sqlx::query(
        r#"
        INSERT INTO bank_rates (bank_name, bank_class, dollar_buy_rate, dollar_sell_rate, updated_at)
        VALUES ($1, $2, $3, $4, NOW())
        ON CONFLICT (bank_class)
        DO UPDATE SET
            bank_name = EXCLUDED.bank_name,
            dollar_buy_rate = EXCLUDED.dollar_buy_rate,
            dollar_sell_rate = EXCLUDED.dollar_sell_rate,
            updated_at = NOW()
        "#,
    )
    .bind(&rate.bank_name)
    .bind(&rate.bank_class)
    .bind(rate.dollar_buy_rate)
    .bind(rate.dollar_sell_rate)
    .execute(pool)
    .await;

    match result {
        Ok(_) => {
            info!(
                "Updated {}: buy={:.2} sell={:.2}",
                rate.bank_class, rate.dollar_buy_rate, rate.dollar_sell_rate
            );
            // Log the change for history
            let _ = sqlx::query(
                "INSERT INTO bank_rates_log (bank_name, bank_class, dollar_buy_rate, dollar_sell_rate) VALUES ($1, $2, $3, $4)",
            )
            .bind(&rate.bank_name)
            .bind(&rate.bank_class)
            .bind(rate.dollar_buy_rate)
            .bind(rate.dollar_sell_rate)
            .execute(pool)
            .await;
        }
        Err(e) => error!("Failed to update {}: {}", rate.bank_class, e),
    }
}

async fn update_all_rates(pool: &PgPool) {
    let client = Client::new();

    let (banreservas, bhd, popular) = tokio::join!(
        fetchers::fetch_banreservas(&client),
        fetchers::fetch_bhd(&client),
        fetchers::fetch_popular(&client),
    );

    for rate in [banreservas, bhd, popular].into_iter().flatten() {
        upsert_rate(pool, &rate).await;
    }
}

// --- Background task ---

async fn rate_updater(pool: PgPool, interval_minutes: u64) {
    let mut ticker = interval(Duration::from_secs(interval_minutes * 60));
    loop {
        ticker.tick().await;
        info!("Updating bank rates...");
        update_all_rates(&pool).await;
        info!("Bank rates update complete.");
    }
}

// --- Entry point ---

#[shuttle_runtime::main]
async fn rocket(
    #[shuttle_shared_db::Postgres] pool: PgPool,
) -> shuttle_rocket::ShuttleRocket {
    // Run migrations
    sqlx::migrate!()
        .run(&pool)
        .await
        .expect("Failed to run migrations");

    // Initial rate fetch
    info!("Fetching initial bank rates...");
    update_all_rates(&pool).await;

    // Spawn background updater (every 30 minutes)
    let updater_pool = pool.clone();
    tokio::spawn(rate_updater(updater_pool, 30));

    let rocket = rocket::build()
        .manage(pool)
        .mount("/", routes![health, get_rates, get_rate_by_bank]);

    Ok(rocket.into())
}
