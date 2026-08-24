<?php
/**
 * Mixed-language fixture: PHP / WordPress-shaped plugin.
 */
class Mixed_Plugin {
    public function boot() {
        add_action('init', [$this, 'boot']);
        add_filter('the_content', [$this, 'filter_content']);
    }

    public function filter_content($content) {
        return $content;
    }
}

function mixed_plugin_helper() {
    return get_option('mixed_plugin');
}
