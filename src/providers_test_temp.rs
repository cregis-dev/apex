    #[test]
    fn registry_respects_protocol_override() {
        let registry = ProviderRegistry::new();
        let channel = Channel {
            name: "c".to_string(),
            provider_type: ProviderType::Minimax, // Usually uses DefaultAdapter
            base_url: "https://api.minimax.io/anthropic".to_string(),
            api_key: "key".to_string(),
            protocol: Some("anthropic".to_string()),
            headers: None,
            model_map: None,
            timeouts: None,
        };
        let headers = HeaderMap::new();
        let prepared = prepare_request(
            &registry,
            &channel,
            RouteKind::Anthropic, // Incoming request is Anthropic
            &channel.base_url,
            "/v1/messages",
            None,
            &headers,
            &Bytes::from("{}"),
        )
        .unwrap();

        // Should use AnthropicAdapter which sets x-api-key
        assert!(prepared.headers.get("x-api-key").is_some());
        // DefaultAdapter would set Authorization
        assert!(prepared.headers.get("authorization").is_none());
        // Path should not be remapped to chat/completions
        assert!(prepared.url.as_str().contains("/v1/messages"));
    }
}
