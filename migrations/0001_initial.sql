CREATE TABLE IF NOT EXISTS bank_rates (
    id SERIAL PRIMARY KEY,
    bank_name VARCHAR(100) NOT NULL,
    bank_class VARCHAR(50) NOT NULL UNIQUE,
    dollar_buy_rate DOUBLE PRECISION NOT NULL,
    dollar_sell_rate DOUBLE PRECISION NOT NULL,
    updated_at TIMESTAMPTZ DEFAULT NOW(),
    created_at TIMESTAMPTZ DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS bank_rates_log (
    id SERIAL PRIMARY KEY,
    bank_name VARCHAR(100) NOT NULL,
    bank_class VARCHAR(50) NOT NULL,
    dollar_buy_rate DOUBLE PRECISION NOT NULL,
    dollar_sell_rate DOUBLE PRECISION NOT NULL,
    created_at TIMESTAMPTZ DEFAULT NOW()
);
