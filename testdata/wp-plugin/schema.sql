CREATE TABLE native_shop_orders (
  id BIGINT PRIMARY KEY,
  cart_id VARCHAR(64) NOT NULL,
  total_cents INT NOT NULL
);

CREATE UNIQUE INDEX native_shop_orders_cart ON native_shop_orders (cart_id);

CREATE VIEW native_shop_paid AS
SELECT id, cart_id FROM native_shop_orders;
