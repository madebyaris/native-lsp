<?php

class Native_Shop_Checkout {
    public function process($cart_id) {
        $cart = new Native_Shop_Cart();
        return [
            'cart' => $cart_id,
            'items' => $cart->total(),
            'status' => 'ok',
        ];
    }

    public function receipt($order_id) {
        return apply_filters('native_shop_receipt', [
            'order' => $order_id,
            'total' => get_option('native_shop_last_total', 0),
        ]);
    }
}

function native_shop_checkout_ajax() {
    $checkout = new Native_Shop_Checkout();
    wp_die(wp_json_encode($checkout->process(isset($_POST['cart']) ? $_POST['cart'] : '')));
}

add_action('wp_ajax_native_shop_checkout', 'native_shop_checkout_ajax');
