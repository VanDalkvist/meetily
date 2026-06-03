#[cfg(test)]
mod tests {
    use super::RealtimeAssistantConfig;

    #[test]
    fn default_config_is_privacy_off_by_default() {
        let config = RealtimeAssistantConfig::default();

        assert!(!config.enabled);
        assert!(config.require_consent_note);
        assert!(config.throttle_ms > 0);
        assert!(config.window_seconds > 0);
        assert!(config.min_new_chars > 0);
    }
}
