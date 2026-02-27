# Dollar Rates

Dominican Republic bank exchange rate API built with Rust, Rocket, and PostgreSQL. Deployed on [Render](https://render.com/).

## Supported Banks

| Bank | Class | Source |
|------|-------|--------|
| Banreservas | `banreservas` | HTML scraping from [banreservas.com/calculadoras](https://www.banreservas.com/calculadoras/) |
| BHD | `bhd` | JSON API at `backend.bhd.com.do` |
| Banco Popular | `popular` | Hardcoded (pending real source) |

## API Endpoints

### Health Check

```
GET /
```

**Response:**

```json
{
  "status": "ok",
  "service": "dollar-rates"
}
```

### Get All Rates

```
GET /rates
```

Returns current exchange rates for all banks.

**Response:**

```json
{
  "success": true,
  "data": [
    {
      "id": 1,
      "bank_name": "Banreservas",
      "bank_class": "banreservas",
      "dollar_buy_rate": 60.10,
      "dollar_sell_rate": 61.25,
      "updated_at": "2026-02-26T19:00:00Z",
      "created_at": "2026-02-26T18:00:00Z"
    },
    {
      "id": 2,
      "bank_name": "BHD",
      "bank_class": "bhd",
      "dollar_buy_rate": 60.05,
      "dollar_sell_rate": 61.20,
      "updated_at": "2026-02-26T19:00:00Z",
      "created_at": "2026-02-26T18:00:00Z"
    },
    {
      "id": 3,
      "bank_name": "Banco Popular",
      "bank_class": "popular",
      "dollar_buy_rate": 58.54,
      "dollar_sell_rate": 59.02,
      "updated_at": "2026-02-26T19:00:00Z",
      "created_at": "2026-02-26T18:00:00Z"
    }
  ]
}
```

### Get Rate by Bank

```
GET /rates/<bank_class>
```

Returns the exchange rate for a specific bank.

**Parameters:**

| Parameter | Type | Description |
|-----------|------|-------------|
| `bank_class` | `string` | Bank identifier: `banreservas`, `bhd`, or `popular` |

**Response (found):**

```json
{
  "success": true,
  "data": {
    "id": 1,
    "bank_name": "Banreservas",
    "bank_class": "banreservas",
    "dollar_buy_rate": 60.10,
    "dollar_sell_rate": 61.25,
    "updated_at": "2026-02-26T19:00:00Z",
    "created_at": "2026-02-26T18:00:00Z"
  }
}
```

**Response (not found):**

```json
{
  "success": false,
  "message": "Bank 'unknown' not found"
}
```

## Database Schema

### `bank_rates` - Current rates (one row per bank)

| Column | Type | Description |
|--------|------|-------------|
| `id` | `SERIAL` | Primary key |
| `bank_name` | `VARCHAR(100)` | Display name |
| `bank_class` | `VARCHAR(50)` | Unique identifier |
| `dollar_buy_rate` | `DOUBLE PRECISION` | USD buy rate in DOP |
| `dollar_sell_rate` | `DOUBLE PRECISION` | USD sell rate in DOP |
| `updated_at` | `TIMESTAMPTZ` | Last rate update |
| `created_at` | `TIMESTAMPTZ` | Row creation time |

### `bank_rates_log` - Historical audit log

| Column | Type | Description |
|--------|------|-------------|
| `id` | `SERIAL` | Primary key |
| `bank_name` | `VARCHAR(100)` | Display name |
| `bank_class` | `VARCHAR(50)` | Bank identifier |
| `dollar_buy_rate` | `DOUBLE PRECISION` | USD buy rate in DOP |
| `dollar_sell_rate` | `DOUBLE PRECISION` | USD sell rate in DOP |
| `created_at` | `TIMESTAMPTZ` | When the rate was recorded |

## Background Updates

Rates are fetched automatically:
- On startup (initial fetch)
- Every 30 minutes via a background task

All three banks are fetched concurrently using `tokio::join!`. Each rate update is also logged to `bank_rates_log` for historical tracking.

## Tech Stack

- **Rust** with [Rocket](https://rocket.rs/) web framework
- **PostgreSQL** via Render managed database
- **sqlx** for async database queries and migrations
- **reqwest** for HTTP requests to bank sources
- **regex** for HTML parsing (Banreservas)

## Deploy on Render

### Option 1: Blueprint (recommended)

1. Push this repo to GitHub
2. Go to [Render Dashboard](https://dashboard.render.com/) > **Blueprints** > **New Blueprint Instance**
3. Connect your repo — Render will use `render.yaml` to create the web service + Postgres database automatically

### Option 2: Manual

1. Create a **PostgreSQL** database in Render
2. Create a **Web Service** with:
   - **Runtime:** Docker
   - **Environment variable:** `DATABASE_URL` = your Render Postgres internal connection string

## Local Development

```bash
# Set your local Postgres connection
export DATABASE_URL="postgres://user:pass@localhost:5432/dollar_rates"

# Run
cargo run
```

## Environment Variables

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `DATABASE_URL` | Yes | — | PostgreSQL connection string |
| `PORT` | No | `10000` | HTTP server port (Render sets this automatically) |
| `POPULAR_PROXY_URL` | No | Direct to popularenlinea.com | Proxy URL for Popular's SharePoint API (site is behind Incapsula WAF). Set this to a proxy that forwards to `https://popularenlinea.com/_api/web/lists/getbytitle('Rates')/items?$filter=ItemID%20eq%20%271%27` |
