<?php

class Native_Shop_Cart {
    public function items() {
        $raw = get_option('native_shop_cart', []);
        return is_array($raw) ? $raw : [];
    }

    public function add($product_id, $qty = 1) {
        $items = $this->items();
        $items[(string) $product_id] = ($items[(string) $product_id] ?? 0) + (int) $qty;
        update_option('native_shop_cart', $items);
        return $items;
    }

    public function total() {
        $sum = 0;
        foreach ($this->items() as $qty) {
            $sum += (int) $qty;
        }
        return $sum;
    }

    public function render() {
        return '<shop-cart class="cart" id="mini-cart"></shop-cart>';
    }
}
