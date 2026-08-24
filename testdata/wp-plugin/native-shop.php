<?php
/**
 * Plugin Name: Native Shop
 * Description: WordPress-shaped storefront plugin used as a real-workspace LSP fixture.
 * Version: 0.1.0
 */

if (!defined('ABSPATH')) {
    exit;
}

require_once __DIR__ . '/includes/class-plugin.php';
require_once __DIR__ . '/includes/class-cart.php';
require_once __DIR__ . '/includes/class-checkout.php';
require_once __DIR__ . '/admin/class-settings.php';

function native_shop_boot() {
    $plugin = new Native_Shop_Plugin();
    $plugin->boot();
}

add_action('plugins_loaded', 'native_shop_boot');
