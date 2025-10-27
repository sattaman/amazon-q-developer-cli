use serde::Serialize;

#[derive(Serialize)]
struct Batch { batch: Vec<Event> }

#[derive(Serialize)]
struct Event {
    id: String,
    timestamp: String,
    #[serde(rename = "type")]
    event_type: String,
    body: serde_json::Value,
}

#[tokio::test]
async fn test_langfuse_connection() {
    let trace_id = uuid::Uuid::new_v4().to_string();
    let batch = Batch {
        batch: vec![Event {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            event_type: "trace-create".to_string(),
            body: serde_json::json!({"id": trace_id, "name": "rust_connection_test"}),
        }],
    };

    let resp = reqwest::Client::new()
        .post("http://localhost:3000/api/public/ingestion")
        .basic_auth("pk-lf-90d38562-6e32-4b81-92cb-ff01dc8b99d8", Some("sk-lf-355fb37d-0f10-4e8f-88eb-ab7bfe47baeb"))
        .json(&batch)
        .send()
        .await
        .expect("Failed to send request");

    let status = resp.status();
    let body = resp.text().await.expect("Failed to read response");
    
    println!("Status: {}", status);
    println!("Response: {}", body);
    println!("Trace URL: http://localhost:3000/traces/{}", trace_id);
    
    assert!(status == 207 || status.is_success(), "Expected 207 or 2xx, got {}", status);
    assert!(body.contains("successes") || body.contains("errors"), "Unexpected response format");
}
