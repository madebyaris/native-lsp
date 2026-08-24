<?php

class Native_Shop_Settings {
    private static $instance;

    public static function instance() {
        if (!self::$instance) {
            self::$instance = new self();
        }
        return self::$instance;
    }

    public function boot() {
        add_action('admin_menu', [$this, 'menu']);
        add_action('admin_init', [$this, 'register']);
    }

    public function menu() {
        add_options_page('Native Shop', 'Native Shop', 'manage_options', 'native-shop', [$this, 'render']);
    }

    public function register() {
        register_setting('native_shop', 'native_shop_currency');
    }

    public function render() {
        echo '<div class="wrap" id="native-shop-settings"><h1>Native Shop</h1></div>';
    }
}
