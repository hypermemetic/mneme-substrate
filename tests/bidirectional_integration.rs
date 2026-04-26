//! Integration tests for bidirectional communication in Plexus RPC
//!
//! These tests verify end-to-end bidirectional flows including:
//! - Request serialization in `PlexusStreamItem::Request`
//! - Response handling through global registry and direct channels
//! - Timeout scenarios
//! - Cancellation handling
//! - Custom request/response type handling
//!
//! Note: Interactive activation tests (wizard, delete, confirm) are in the
//! activation module itself (mneme-substrate/src/activations/interactive/activation.rs)
//! since those methods are private to the module.

use async_stream::stream;
use futures::StreamExt;
use plexus_core::plexus::bidirectional::{
    auto_respond_channel, create_test_standard_channel, BidirChannel, BidirError, SelectOption,
    StandardBidirChannel, StandardRequest, StandardResponse,
};
use plexus_core::plexus::types::PlexusStreamItem;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::timeout;

// =============================================================================
// Request Serialization Tests
// =============================================================================

/// Test that `PlexusStreamItem::Request` is properly serialized with all required fields
#[tokio::test]
async fn test_request_serialization_format() {
    let (ctx, mut rx) = create_test_standard_channel();

    // Spawn request in background (it will timeout, but we want to capture the item)
    let ctx_clone = ctx.clone();
    tokio::spawn(async move {
        let _ = ctx_clone
            .request_with_timeout(
                StandardRequest::Confirm {
                    message: "Test confirmation?".into(),
                    default: Some(true),
                },
                Duration::from_millis(50),
            )
            .await;
    });

    // Receive the request item
    let item = timeout(Duration::from_millis(100), rx.recv())
        .await
        .expect("Should receive request")
        .expect("Channel should not be closed");

    // Verify it's a Request variant
    match item {
        PlexusStreamItem::Request {
            request_id,
            request_data,
            timeout_ms,
        } => {
            // Verify request_id is a valid UUID format
            assert!(
                request_id.len() == 36,
                "request_id should be UUID format: {request_id}"
            );

            // Verify timeout_ms is set
            assert_eq!(timeout_ms, 50);

            // Verify request_data contains the serialized StandardRequest
            let req: StandardRequest = serde_json::from_value(request_data.clone())
                .expect("request_data should deserialize to StandardRequest");

            match req {
                StandardRequest::Confirm { message, default } => {
                    assert_eq!(message, "Test confirmation?");
                    assert_eq!(default, Some(true));
                }
                _ => panic!("Expected Confirm request"),
            }

            // Verify JSON structure
            assert_eq!(request_data["type"], "confirm");
            assert_eq!(request_data["message"], "Test confirmation?");
            assert_eq!(request_data["default"], true);
        }
        other => panic!("Expected Request item, got {other:?}"),
    }
}

/// Test that prompt request is properly serialized
#[tokio::test]
async fn test_prompt_request_serialization() {
    let (ctx, mut rx) = create_test_standard_channel();

    tokio::spawn(async move {
        let _ = ctx
            .request_with_timeout(
                StandardRequest::Prompt {
                    message: "Enter name:".into(),
                    default: Some("default_value".into()),
                    placeholder: Some("Type here...".into()),
                },
                Duration::from_millis(50),
            )
            .await;
    });

    let item = rx.recv().await.expect("Should receive item");

    if let PlexusStreamItem::Request { request_data, .. } = item {
        assert_eq!(request_data["type"], "prompt");
        assert_eq!(request_data["message"], "Enter name:");
        assert_eq!(request_data["default"], "default_value");
        assert_eq!(request_data["placeholder"], "Type here...");
    } else {
        panic!("Expected Request item");
    }
}

/// Test that select request is properly serialized with options
#[tokio::test]
async fn test_select_request_serialization() {
    let (ctx, mut rx) = create_test_standard_channel();

    tokio::spawn(async move {
        let options = vec![
            SelectOption::new("opt1", "Option 1").with_description("First option"),
            SelectOption::new("opt2", "Option 2"),
        ];
        let _ = ctx
            .request_with_timeout(
                StandardRequest::Select {
                    message: "Choose one:".into(),
                    options,
                    multi_select: true,
                },
                Duration::from_millis(50),
            )
            .await;
    });

    let item = rx.recv().await.expect("Should receive item");

    if let PlexusStreamItem::Request { request_data, .. } = item {
        assert_eq!(request_data["type"], "select");
        assert_eq!(request_data["message"], "Choose one:");
        assert_eq!(request_data["multi_select"], true);

        let options = request_data["options"].as_array().expect("options array");
        assert_eq!(options.len(), 2);
        assert_eq!(options[0]["value"], "opt1");
        assert_eq!(options[0]["label"], "Option 1");
        assert_eq!(options[0]["description"], "First option");
        assert_eq!(options[1]["value"], "opt2");
        assert_eq!(options[1]["label"], "Option 2");
        assert!(options[1].get("description").is_none());
    } else {
        panic!("Expected Request item");
    }
}

// =============================================================================
// Request/Response Flow Tests
// =============================================================================

/// Test complete request/response flow with direct channel
#[tokio::test]
async fn test_direct_channel_request_response_flow() {
    let (tx, mut rx) = mpsc::channel::<PlexusStreamItem>(32);
    let channel: Arc<StandardBidirChannel> = Arc::new(BidirChannel::new_direct(
        tx,
        true,
        vec!["test".into()],
        "test-hash".into(),
    ));

    // Spawn request
    let channel_for_request = channel.clone();
    let handle = tokio::spawn(async move { channel_for_request.confirm("Delete file?").await });

    // Receive and respond
    if let Some(PlexusStreamItem::Request {
        request_id,
        request_data,
        ..
    }) = rx.recv().await
    {
        // Verify request
        let req: StandardRequest = serde_json::from_value(request_data).unwrap();
        assert!(matches!(req, StandardRequest::Confirm { message, .. } if message == "Delete file?"));

        // Send response
        let response = StandardResponse::<serde_json::Value>::Confirmed { value: true };
        channel
            .handle_response(request_id, serde_json::to_value(&response).unwrap())
            .expect("handle_response should succeed");
    }

    // Verify result
    let result = handle.await.unwrap();
    assert!(result.unwrap());
}

/// Test prompt request/response flow
#[tokio::test]
async fn test_prompt_request_response_flow() {
    let (tx, mut rx) = mpsc::channel(32);
    let channel: Arc<StandardBidirChannel> =
        Arc::new(BidirChannel::new_direct(tx, true, vec![], "hash".into()));

    let channel_for_request = channel.clone();
    let handle = tokio::spawn(async move { channel_for_request.prompt("Enter name:").await });

    if let Some(PlexusStreamItem::Request { request_id, .. }) = rx.recv().await {
        channel
            .handle_response(
                request_id,
                serde_json::to_value(&StandardResponse::<serde_json::Value>::Text {
                    value: serde_json::Value::String("John Doe".into()),
                })
                .unwrap(),
            )
            .unwrap();
    }

    let result = handle.await.unwrap().unwrap();
    assert_eq!(result, "John Doe");
}

/// Test select request/response flow
#[tokio::test]
async fn test_select_request_response_flow() {
    let (tx, mut rx) = mpsc::channel(32);
    let channel: Arc<StandardBidirChannel> =
        Arc::new(BidirChannel::new_direct(tx, true, vec![], "hash".into()));

    let channel_for_request = channel.clone();
    let handle = tokio::spawn(async move {
        let options = vec![
            SelectOption::new("dev", "Development"),
            SelectOption::new("prod", "Production"),
        ];
        channel_for_request.select("Environment:", options).await
    });

    if let Some(PlexusStreamItem::Request { request_id, .. }) = rx.recv().await {
        channel
            .handle_response(
                request_id,
                serde_json::to_value(&StandardResponse::<serde_json::Value>::Selected {
                    values: vec![serde_json::Value::String("prod".into())],
                })
                .unwrap(),
            )
            .unwrap();
    }

    let result = handle.await.unwrap().unwrap();
    assert_eq!(result, vec!["prod".to_string()]);
}

/// Test cancelled response handling
#[tokio::test]
async fn test_cancelled_response_handling() {
    let (tx, mut rx) = mpsc::channel(32);
    let channel: Arc<StandardBidirChannel> =
        Arc::new(BidirChannel::new_direct(tx, true, vec![], "hash".into()));

    let channel_for_request = channel.clone();
    let handle = tokio::spawn(async move { channel_for_request.confirm("Proceed?").await });

    if let Some(PlexusStreamItem::Request { request_id, .. }) = rx.recv().await {
        channel
            .handle_response(
                request_id,
                serde_json::to_value(&StandardResponse::<serde_json::Value>::Cancelled).unwrap(),
            )
            .unwrap();
    }

    let result = handle.await.unwrap();
    assert!(matches!(result, Err(BidirError::Cancelled)));
}

// =============================================================================
// Timeout Tests
// =============================================================================

/// Test that request times out and stream continues
#[tokio::test]
async fn test_timeout_handling() {
    let (tx, _rx) = mpsc::channel(32);
    let channel: StandardBidirChannel =
        BidirChannel::new_direct(tx, true, vec!["test".into()], "hash".into());

    let start = std::time::Instant::now();
    let result = channel
        .request_with_timeout(
            StandardRequest::Confirm {
                message: "Test?".into(),
                default: None,
            },
            Duration::from_millis(100),
        )
        .await;

    let elapsed = start.elapsed();

    // Should have timed out
    assert!(
        matches!(result, Err(BidirError::Timeout(100))),
        "Expected Timeout error, got {result:?}"
    );

    // Should have taken approximately 100ms
    assert!(
        elapsed >= Duration::from_millis(90) && elapsed < Duration::from_millis(200),
        "Timeout should take ~100ms, took {elapsed:?}"
    );
}

/// Test that short timeout doesn't block
#[tokio::test]
async fn test_short_timeout() {
    let (tx, _rx) = mpsc::channel(32);
    let channel: StandardBidirChannel =
        BidirChannel::new_direct(tx, true, vec![], "hash".into());

    let start = std::time::Instant::now();
    let result = channel
        .request_with_timeout(
            StandardRequest::Prompt {
                message: "Quick?".into(),
                default: None,
                placeholder: None,
            },
            Duration::from_millis(10),
        )
        .await;

    let elapsed = start.elapsed();

    assert!(matches!(result, Err(BidirError::Timeout(10))));
    assert!(elapsed < Duration::from_millis(100));
}

/// Test multiple sequential timeouts don't accumulate state
#[tokio::test]
async fn test_sequential_timeouts_clean_state() {
    let (tx, _rx) = mpsc::channel(32);
    let channel: StandardBidirChannel =
        BidirChannel::new_direct(tx, true, vec![], "hash".into());

    // First timeout
    let _ = channel
        .request_with_timeout(
            StandardRequest::Confirm {
                message: "First?".into(),
                default: None,
            },
            Duration::from_millis(10),
        )
        .await;

    // Second timeout should work independently
    let result = channel
        .request_with_timeout(
            StandardRequest::Confirm {
                message: "Second?".into(),
                default: None,
            },
            Duration::from_millis(10),
        )
        .await;

    assert!(matches!(result, Err(BidirError::Timeout(10))));
}

// =============================================================================
// Not Supported Tests
// =============================================================================

/// Test that non-bidirectional channel returns `NotSupported`
#[tokio::test]
async fn test_not_supported_error() {
    let (tx, _rx) = mpsc::channel(32);
    let channel: StandardBidirChannel = BidirChannel::new_direct(
        tx,
        false, // bidirectional NOT supported
        vec![],
        "hash".into(),
    );

    let result = channel.confirm("Test?").await;
    assert!(
        matches!(result, Err(BidirError::NotSupported)),
        "Expected NotSupported, got {result:?}"
    );

    let result = channel.prompt("Test?").await;
    assert!(matches!(result, Err(BidirError::NotSupported)));

    let result = channel
        .select("Test?", vec![SelectOption::new("a", "A")])
        .await;
    assert!(matches!(result, Err(BidirError::NotSupported)));
}

/// Test `is_bidirectional()` method
#[tokio::test]
async fn test_is_bidirectional_flag() {
    let (tx1, _rx1) = mpsc::channel(32);
    let channel_enabled: StandardBidirChannel =
        BidirChannel::new_direct(tx1, true, vec![], "hash".into());
    assert!(channel_enabled.is_bidirectional());

    let (tx2, _rx2) = mpsc::channel(32);
    let channel_disabled: StandardBidirChannel =
        BidirChannel::new_direct(tx2, false, vec![], "hash".into());
    assert!(!channel_disabled.is_bidirectional());
}

// =============================================================================
// Type Mismatch Tests
// =============================================================================

/// Test that wrong response type causes `TypeMismatch` error
#[tokio::test]
async fn test_type_mismatch_on_confirm() {
    let (tx, mut rx) = mpsc::channel(32);
    let channel: Arc<StandardBidirChannel> =
        Arc::new(BidirChannel::new_direct(tx, true, vec![], "hash".into()));

    let channel_for_request = channel.clone();
    let handle = tokio::spawn(async move { channel_for_request.confirm("Test?").await });

    // Respond with wrong type (Text instead of Confirmed)
    if let Some(PlexusStreamItem::Request { request_id, .. }) = rx.recv().await {
        channel
            .handle_response(
                request_id,
                serde_json::to_value(&StandardResponse::<serde_json::Value>::Text {
                    value: serde_json::Value::String("wrong".into()),
                })
                .unwrap(),
            )
            .unwrap();
    }

    let result = handle.await.unwrap();
    assert!(
        matches!(result, Err(BidirError::TypeMismatch { .. })),
        "Expected TypeMismatch, got {result:?}"
    );
}

/// Test that wrong response type causes `TypeMismatch` error on prompt
#[tokio::test]
async fn test_type_mismatch_on_prompt() {
    let (tx, mut rx) = mpsc::channel(32);
    let channel: Arc<StandardBidirChannel> =
        Arc::new(BidirChannel::new_direct(tx, true, vec![], "hash".into()));

    let channel_for_request = channel.clone();
    let handle = tokio::spawn(async move { channel_for_request.prompt("Name?").await });

    // Respond with wrong type (Confirmed instead of Text)
    if let Some(PlexusStreamItem::Request { request_id, .. }) = rx.recv().await {
        channel
            .handle_response(
                request_id,
                serde_json::to_value(&StandardResponse::<serde_json::Value>::Confirmed {
                    value: true,
                })
                .unwrap(),
            )
            .unwrap();
    }

    let result = handle.await.unwrap();
    assert!(matches!(result, Err(BidirError::TypeMismatch { .. })));
}

// =============================================================================
// Auto-Respond Channel Tests
// =============================================================================

/// Test `auto_respond_channel` with custom response function
#[tokio::test]
async fn test_auto_respond_channel_custom() {
    let ctx = auto_respond_channel(|req: &StandardRequest| match req {
        StandardRequest::Confirm { message, .. } => {
            if message.contains("dangerous") {
                StandardResponse::Confirmed { value: false }
            } else {
                StandardResponse::Confirmed { value: true }
            }
        }
        StandardRequest::Prompt { .. } => StandardResponse::Text {
            value: serde_json::Value::String("auto-response".into()),
        },
        StandardRequest::Select { options, .. } => StandardResponse::Selected {
            values: vec![options.last().unwrap().value.clone()],
        },
        StandardRequest::Custom { data } => StandardResponse::Custom { data: data.clone() },
    });

    // Test confirm logic
    assert!(ctx.confirm("Safe action?").await.unwrap());
    assert!(!ctx.confirm("This is dangerous!").await.unwrap());

    // Test prompt
    assert_eq!(ctx.prompt("Name?").await.unwrap(), "auto-response");

    // Test select (picks last option)
    let options = vec![
        SelectOption::new("first", "First"),
        SelectOption::new("last", "Last"),
    ];
    let selected = ctx.select("Pick:", options).await.unwrap();
    assert_eq!(selected, vec!["last".to_string()]);
}

/// Test multiple concurrent requests with `auto_respond_channel`
#[tokio::test]
async fn test_concurrent_auto_responses() {
    let ctx = auto_respond_channel(|req: &StandardRequest| match req {
        StandardRequest::Confirm { .. } => StandardResponse::Confirmed { value: true },
        StandardRequest::Prompt { message, .. } => StandardResponse::Text {
            value: serde_json::Value::String(message.clone()),
        },
        StandardRequest::Select { .. } => StandardResponse::Selected {
            values: vec![serde_json::Value::String("selected".into())],
        },
        StandardRequest::Custom { data } => StandardResponse::Custom { data: data.clone() },
    });

    // Spawn multiple concurrent requests
    let ctx1 = ctx.clone();
    let ctx2 = ctx.clone();
    let ctx3 = ctx.clone();

    let (r1, r2, r3) = tokio::join!(
        ctx1.confirm("Request 1?"),
        ctx2.prompt("Request 2"),
        ctx3.select("Request 3", vec![SelectOption::new("a", "A")]),
    );

    assert!(r1.unwrap());
    assert_eq!(r2.unwrap(), "Request 2");
    assert_eq!(r3.unwrap(), vec!["selected".to_string()]);
}

// =============================================================================
// Custom Request/Response Type Tests
// =============================================================================

/// Custom request type for testing
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
enum CustomRequest {
    GetValue { key: String },
    SetValue { key: String, value: i32 },
}

/// Custom response type for testing
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
enum CustomResponse {
    Value { data: i32 },
    Acknowledged,
    NotFound,
}

/// Test custom request/response types
#[tokio::test]
async fn test_custom_request_response_types() {
    let (tx, mut rx) = mpsc::channel::<PlexusStreamItem>(32);
    let channel: Arc<BidirChannel<CustomRequest, CustomResponse>> =
        Arc::new(BidirChannel::new_direct(tx, true, vec![], "hash".into()));

    let channel_for_request = channel.clone();
    let handle = tokio::spawn(async move {
        channel_for_request
            .request(CustomRequest::GetValue {
                key: "test_key".into(),
            })
            .await
    });

    // Receive and respond
    if let Some(PlexusStreamItem::Request {
        request_id,
        request_data,
        ..
    }) = rx.recv().await
    {
        // Verify custom request serialization
        assert_eq!(request_data["type"], "get_value");
        assert_eq!(request_data["key"], "test_key");

        // Send custom response
        channel
            .handle_response(
                request_id,
                serde_json::to_value(&CustomResponse::Value { data: 42 }).unwrap(),
            )
            .unwrap();
    }

    let result = handle.await.unwrap().unwrap();
    assert_eq!(result, CustomResponse::Value { data: 42 });
}

// =============================================================================
// Global Registry Tests (simulating MCP transport)
// =============================================================================

/// Test that global registry mode works for routing responses
#[tokio::test]
async fn test_global_registry_request_response() {
    use plexus_core::plexus::bidirectional::{handle_pending_response, is_request_pending};

    let (tx, mut rx) = mpsc::channel::<PlexusStreamItem>(32);

    // Create channel using global registry (default behavior)
    let channel: StandardBidirChannel =
        BidirChannel::new(tx, true, vec!["mcp".into()], "hash".into());

    // Spawn request
    let handle = tokio::spawn(async move { channel.confirm("MCP confirm?").await });

    // Give time for request to be sent
    tokio::time::sleep(Duration::from_millis(10)).await;

    // Receive the request
    if let Some(PlexusStreamItem::Request { request_id, .. }) = rx.recv().await {
        // Verify request is pending in global registry
        assert!(
            is_request_pending(&request_id),
            "Request should be pending in global registry"
        );

        // Simulate MCP _plexus_respond tool call
        handle_pending_response(
            &request_id,
            serde_json::to_value(&StandardResponse::<serde_json::Value>::Confirmed { value: true })
            .unwrap(),
        )
        .expect("handle_pending_response should succeed");

        // Request should no longer be pending
        assert!(
            !is_request_pending(&request_id),
            "Request should be removed after response"
        );
    }

    // Verify result
    let result = handle.await.unwrap();
    assert!(result.unwrap());
}

/// Test that unknown request ID returns error
#[tokio::test]
async fn test_global_registry_unknown_request() {
    use plexus_core::plexus::bidirectional::handle_pending_response;

    let result = handle_pending_response("nonexistent-id", serde_json::json!({"test": true}));
    assert!(
        matches!(result, Err(BidirError::UnknownRequest)),
        "Should return UnknownRequest for unknown ID"
    );
}

// =============================================================================
// Multi-Step Workflow Simulation Tests
// =============================================================================

/// Simulate a multi-step interactive workflow (like wizard) using bidirectional channel
/// This demonstrates how the interactive activation pattern works
#[tokio::test]
async fn test_multi_step_workflow_simulation() {

    // Create channel with auto-responses
    let ctx = auto_respond_channel(|req: &StandardRequest| match req {
        StandardRequest::Prompt { message, .. } => {
            if message.contains("name") {
                StandardResponse::Text {
                    value: serde_json::Value::String("my-project".into()),
                }
            } else {
                StandardResponse::Text {
                    value: serde_json::Value::String("default".into()),
                }
            }
        }
        StandardRequest::Select { options, .. } => StandardResponse::Selected {
            values: vec![options[0].value.clone()],
        },
        StandardRequest::Confirm { .. } => StandardResponse::Confirmed { value: true },
        StandardRequest::Custom { data } => StandardResponse::Custom { data: data.clone() },
    });

    // Simulate a wizard-like workflow
    let workflow = stream! {
        yield "started".to_string();

        // Step 1: Get name via prompt
        let name = ctx.prompt("Enter project name:").await;
        match name {
            Ok(n) => yield format!("name:{}", n),
            Err(_) => {
                yield "error:name".to_string();
                return;
            }
        }

        // Step 2: Select option
        let options = vec![
            SelectOption::new("opt1", "Option 1"),
            SelectOption::new("opt2", "Option 2"),
        ];
        let selected = ctx.select("Choose option:", options).await;
        match selected {
            Ok(s) => yield format!("selected:{}", s.join(",")),
            Err(_) => {
                yield "error:select".to_string();
                return;
            }
        }

        // Step 3: Confirm
        let confirmed = ctx.confirm("Proceed?").await;
        match confirmed {
            Ok(true) => yield "confirmed".to_string(),
            Ok(false) => yield "declined".to_string(),
            Err(_) => yield "error:confirm".to_string(),
        }

        yield "done".to_string();
    };

    let events: Vec<String> = workflow.collect().await;

    assert_eq!(events[0], "started");
    assert_eq!(events[1], "name:my-project");
    assert_eq!(events[2], "selected:opt1");
    assert_eq!(events[3], "confirmed");
    assert_eq!(events[4], "done");
}

/// Simulate workflow with cancellation mid-way
#[tokio::test]
async fn test_multi_step_workflow_cancellation() {

    // Auto-respond with cancellation at select step
    let ctx = auto_respond_channel(|req: &StandardRequest| match req {
        StandardRequest::Prompt { .. } => StandardResponse::Text {
            value: serde_json::Value::String("test".into()),
        },
        StandardRequest::Select { .. } => StandardResponse::Cancelled,
        StandardRequest::Confirm { .. } => StandardResponse::Confirmed { value: true },
        StandardRequest::Custom { data } => StandardResponse::Custom { data: data.clone() },
    });

    let workflow = stream! {
        yield "started".to_string();

        let _ = ctx.prompt("Name?").await.unwrap();
        yield "got_name".to_string();

        // This will be cancelled
        let result = ctx.select("Choose:", vec![SelectOption::new("a", "A")]).await;
        match result {
            Err(BidirError::Cancelled) => yield "cancelled".to_string(),
            _ => yield "unexpected".to_string(),
        }
    };

    let events: Vec<String> = workflow.collect().await;

    assert_eq!(events, vec!["started", "got_name", "cancelled"]);
}

// =============================================================================
// Edge Case Tests
// =============================================================================

/// Test handling of malformed JSON response
#[tokio::test]
async fn test_malformed_response_handling() {
    let (tx, mut rx) = mpsc::channel(32);
    let channel: Arc<StandardBidirChannel> =
        Arc::new(BidirChannel::new_direct(tx, true, vec![], "hash".into()));

    let channel_for_request = channel.clone();
    let handle = tokio::spawn(async move { channel_for_request.confirm("Test?").await });

    if let Some(PlexusStreamItem::Request { request_id, .. }) = rx.recv().await {
        // Send malformed response (not a valid StandardResponse)
        let result = channel.handle_response(request_id, serde_json::json!({"invalid": "data"}));

        // Should fail to deserialize
        assert!(matches!(result, Err(BidirError::TypeMismatch { .. })));
    }

    // Original request should fail (channel closed or similar)
    let result = handle.await.unwrap();
    assert!(result.is_err());
}

/// Test that response to wrong request ID fails
#[tokio::test]
async fn test_wrong_request_id() {
    let (tx, mut rx) = mpsc::channel(32);
    let channel: Arc<StandardBidirChannel> =
        Arc::new(BidirChannel::new_direct(tx, true, vec![], "hash".into()));

    let channel_for_request = channel.clone();
    tokio::spawn(async move {
        let _ = channel_for_request
            .request_with_timeout(
                StandardRequest::Confirm {
                    message: "Test?".into(),
                    default: None,
                },
                Duration::from_millis(100),
            )
            .await;
    });

    if let Some(PlexusStreamItem::Request { .. }) = rx.recv().await {
        // Respond with wrong request ID
        let result = channel.handle_response(
            "wrong-id-12345".to_string(),
            serde_json::to_value(&StandardResponse::<serde_json::Value>::Confirmed {
                value: true,
            })
            .unwrap(),
        );

        assert!(
            matches!(result, Err(BidirError::UnknownRequest)),
            "Expected UnknownRequest for wrong ID"
        );
    }
}

/// Test channel metadata (provenance, `plexus_hash`)
#[tokio::test]
async fn test_channel_metadata() {
    let (tx, _rx) = mpsc::channel(32);
    let channel: StandardBidirChannel = BidirChannel::new_direct(
        tx,
        true,
        vec!["substrate".into(), "interactive".into(), "wizard".into()],
        "abc123hash".into(),
    );

    assert_eq!(
        channel.provenance(),
        &["substrate", "interactive", "wizard"]
    );
    assert_eq!(channel.plexus_hash(), "abc123hash");
}

/// Test that dropping receiver causes request to fail appropriately
#[tokio::test]
async fn test_receiver_dropped() {
    let (tx, rx) = mpsc::channel(32);
    let channel: StandardBidirChannel =
        BidirChannel::new_direct(tx, true, vec![], "hash".into());

    // Drop the receiver immediately
    drop(rx);

    // Request should fail since channel is closed
    let result = channel.confirm("Test?").await;

    // Should get a transport error (send failed)
    assert!(
        matches!(result, Err(BidirError::Transport(_))),
        "Expected Transport error, got {result:?}"
    );
}
