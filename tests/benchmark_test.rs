use reqwest::Client;
use serde_json::json;
use std::error::Error;
use std::time::Instant;
use tokio::task::JoinSet;

const GATEWAY_URL: &str = "http://127.0.0.1:12356";
const VKEY: &str = "test-key";

// Helper to send a request
async fn send_chat_request(client: &Client, messages: Vec<serde_json::Value>) -> Result<(u16, u128), Box<dyn Error + Send + Sync>> {
    let start = Instant::now();
    let res = client
        .post(format!("{}/v1/chat/completions", GATEWAY_URL))
        .header("Authorization", format!("Bearer {}", VKEY))
        .header("Content-Type", "application/json")
        .json(&json!({
            "model": "gemini-2.0-flash",
            "messages": messages
        }))
        .send()
        .await
        .map_err(|e| Box::new(e) as Box<dyn Error + Send + Sync>)?;
    
    let status = res.status().as_u16();
    let _text = res.text().await.map_err(|e| Box::new(e) as Box<dyn Error + Send + Sync>)?; // Ensure body is read
    let duration = start.elapsed().as_millis();
    
    Ok((status, duration))
}

#[tokio::test]
#[ignore]
async fn test_continuous_conversation() -> Result<(), Box<dyn Error>> {
    let client = Client::new();
    println!("Starting Continuous Conversation Test...");

    let mut messages = vec![
        json!({"role": "user", "content": "Hello, I am testing the gateway. Remember that my favorite number is 42."})
    ];

    // Turn 1
    println!("Turn 1: Initial greeting...");
    let (status, duration) = send_chat_request(&client, messages.clone())
        .await
        .map_err(|e| e as Box<dyn Error>)?;
    assert_eq!(status, 200);
    println!("Turn 1 completed in {}ms", duration);
    
    // Simulate assistant response
    messages.push(json!({"role": "assistant", "content": "Hello! I've noted that your favorite number is 42."}));
    
    // Turn 2
    println!("Turn 2: Follow-up question...");
    messages.push(json!({"role": "user", "content": "What is my favorite number?"}));
    let (status, duration) = send_chat_request(&client, messages.clone())
        .await
        .map_err(|e| e as Box<dyn Error>)?;
    assert_eq!(status, 200);
    println!("Turn 2 completed in {}ms", duration);

    // Turn 3
    println!("Turn 3: Another follow-up...");
    messages.push(json!({"role": "assistant", "content": "Your favorite number is 42."}));
    messages.push(json!({"role": "user", "content": "Thanks! Tell me a short joke."}));
    let (status, duration) = send_chat_request(&client, messages.clone())
        .await
        .map_err(|e| e as Box<dyn Error>)?;
    assert_eq!(status, 200);
    println!("Turn 3 completed in {}ms", duration);

    Ok(())
}

#[tokio::test]
#[ignore]
async fn test_concurrency_stability() -> Result<(), Box<dyn Error>> {
    let client = Client::new();
    let concurrency = 5; // Parallel requests
    let mut set = JoinSet::new();

    println!("Starting Concurrency Test ({} requests)...", concurrency);
    let start_total = Instant::now();

    for i in 0..concurrency {
        let client = client.clone();
        set.spawn(async move {
            let messages = vec![
                json!({"role": "user", "content": format!("Hello from thread {}, just say 'hi'", i)})
            ];
            send_chat_request(&client, messages).await
        });
    }

    let mut success_count = 0;
    let mut total_latency = 0;

    while let Some(res) = set.join_next().await {
        match res {
            Ok(Ok((status, duration))) => {
                if status == 200 {
                    success_count += 1;
                    total_latency += duration;
                } else {
                    println!("Request failed with status: {}", status);
                }
            }
            Ok(Err(e)) => println!("Request error: {}", e),
            Err(e) => println!("Join error: {}", e),
        }
    }

    let total_duration = start_total.elapsed().as_millis();
    let avg_latency = if success_count > 0 { total_latency / success_count as u128 } else { 0 };

    println!("Concurrency Test Results:");
    println!("Total Requests: {}", concurrency);
    println!("Successful: {}", success_count);
    println!("Total Wall Time: {}ms", total_duration);
    println!("Average Request Latency: {}ms", avg_latency);

    assert_eq!(success_count, concurrency, "Not all concurrent requests succeeded");
    Ok(())
}
