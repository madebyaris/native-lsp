<?php
/**
 * Plugin Name: Native LSP Fixture
 * Description: Tiny WordPress-shaped plugin used as a manual LSP fixture.
 */

class Native_Lsp_Fixture {
    public function boot() {
        add_action('init', [$this, 'boot']);
        add_filter('the_content', [$this, 'filter_content']);
    }

    public function filter_content($content) {
        return $content;
    }
}

function native_lsp_fixture_helper() {
    return get_option('native_lsp_fixture');
}
