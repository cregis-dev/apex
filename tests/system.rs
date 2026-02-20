use futures::StreamExt;
use reqwest::Client;
use serde_json::json;
use std::error::Error;

const GATEWAY_URL: &str = "http://127.0.0.1:12356";
// Note: These VKEYs must match the configuration of the running apex service.
// You can find them in ~/.apex/config.json or by running `apex router list`.
const OPENAI_VKEY: &str = "vk_zfn1SWzWCKLnP411HHywGnAV";
const ANTHROPIC_VKEY: &str = "vk_FP7krRxicme4ht91loT40mYT";

#[tokio::test]
#[ignore]
async fn test_openai_protocol() -> Result<(), Box<dyn Error>> {
    let client = Client::new();
    println!("Testing OpenAI Protocol...");
    let res = client
        .post(format!("{}/v1/chat/completions", GATEWAY_URL))
        .header("Authorization", format!("Bearer {}", OPENAI_VKEY))
        .header("Content-Type", "application/json")
        .json(&json!({
            "model": "gemini-2.0-flash",
            "messages": [{"role": "user", "content": "Hello"}]
        }))
        .send()
        .await?;

    println!("OpenAI Status: {}", res.status());
    assert_eq!(res.status(), 200);
    let body = res.text().await?;
    println!("OpenAI Body: {}", body);
    assert!(body.contains("choices"));
    Ok(())
}

#[tokio::test]
#[ignore]
async fn test_anthropic_protocol() -> Result<(), Box<dyn Error>> {
    let client = Client::new();
    println!("\nTesting Anthropic Protocol...");
    let res = client
        .post(format!("{}/v1/messages", GATEWAY_URL))
        .header("x-api-key", ANTHROPIC_VKEY)
        .header("anthropic-version", "2023-06-01")
        .header("Content-Type", "application/json")
        .json(&json!({
            "model": "claude-3-5-sonnet-20240620",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Hello"}]
        }))
        .send()
        .await?;

    println!("Anthropic Status: {}", res.status());
    assert_eq!(res.status(), 200);
    let body = res.text().await?;
    println!("Anthropic Body: {}", body);
    // Basic validation of Anthropic response format
    assert!(body.contains("content"));
    Ok(())
}

#[tokio::test]
#[ignore]
async fn test_anthropic_protocol_streaming() -> Result<(), Box<dyn Error>> {
    let client = Client::new();
    println!("\nTesting Anthropic Protocol (Streaming)...");
    let res = client
        .post(format!("{}/v1/messages", GATEWAY_URL))
        .header("x-api-key", ANTHROPIC_VKEY)
        .header("anthropic-version", "2023-06-01")
        .header("Content-Type", "application/json")
        .json(&json!({
            "model": "claude-3-5-sonnet-20240620",
            "max_tokens": 1024,
            "messages": [{"role": "user", "content": "Hello"}],
            "stream": true
        }))
        .send()
        .await?;

    println!("Anthropic Streaming Status: {}", res.status());
    assert_eq!(res.status(), 200);

    let mut stream = res.bytes_stream();
    let mut received_data = false;
    while let Some(item) = stream.next().await {
        match item {
            Ok(bytes) => {
                let chunk = String::from_utf8_lossy(&bytes);
                println!("Chunk: {:?}", chunk);
                if !chunk.is_empty() {
                    received_data = true;
                }
            }
            Err(e) => println!("Error: {}", e),
        }
    }
    assert!(received_data, "Should receive streaming data");
    Ok(())
}

#[tokio::test]
#[ignore]
async fn test_auth_header_fallback() -> Result<(), Box<dyn Error>> {
    let client = Client::new();
    println!("\nTesting Auth Header Fallback...");
    // Use standard Authorization header
    let res = client
        .post(format!("{}/v1/chat/completions", GATEWAY_URL))
        .header("Authorization", format!("Bearer {}", OPENAI_VKEY))
        .header("Content-Type", "application/json")
        .json(&json!({
            "model": "gemini-2.0-flash",
            "messages": [{"role": "user", "content": "Hello"}]
        }))
        .send()
        .await?;

    println!("Auth Fallback Status: {}", res.status());
    assert_eq!(res.status(), 200);
    Ok(())
}
