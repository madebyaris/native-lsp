<?php

class Native_Shop_Plugin {
    public function boot() {
        add_action('init', [$this, 'register']);
        add_action('wp_enqueue_scripts', [$this, 'assets']);
        add_filter('the_content', [$this, 'inject_cart']);
        Native_Shop_Settings::instance()->boot();
    }

    public function register() {
        register_post_type('native_product', [
            'public' => true,
            'label' => 'Products',
        ]);
    }

    public function assets() {
        wp_enqueue_script('native-shop-cart', plugin_dir_url(__FILE__) . '../assets/js/cart-widget.js', [], '0.1.0', true);
        wp_enqueue_style('native-shop', plugin_dir_url(__FILE__) . '../assets/css/storefront.css', [], '0.1.0');
    }

    public function inject_cart($content) {
        if (!is_singular('native_product')) {
            return $content;
        }
        $cart = new Native_Shop_Cart();
        return $content . $cart->render();
    }
}
