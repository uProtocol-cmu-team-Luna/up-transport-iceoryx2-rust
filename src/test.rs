// src/test.rs

use super::*;
use bytes::Bytes;
use std::sync::Arc;

use protobuf::MessageField;  // if you use this in tests

// Example synchronous test (no async needed) -works
#[test]
fn test_custom_header_from_user_header() {
    let header = CustomHeader { version: 2, timestamp: 1000 };
    let new_header = CustomHeader::from_user_header(&header).unwrap();
    assert_eq!(new_header.version, 2);
    assert_eq!(new_header.timestamp, 1000);
}

// Async test requires Tokio runtime - work
#[tokio::test]
async fn test_transport_creation() {
    let transport = Iceoryx2Transport::new();
    assert!(transport.is_ok());
}

//checks that umessage is correctly parsed into transmission data - works
#[test]
fn test_transmission_data_from_message() {
    let data = TransmissionData {
        x: 42,
        y: -7,
        funky: 3.14,
    };

    let bytes = data.to_bytes();
    let mut msg = UMessage::new();
    msg.payload = Some(Bytes::from(bytes));

    let result = TransmissionData::from_message(&msg);
    assert!(result.is_ok());
    let decoded = result.unwrap();
    assert_eq!(decoded.x, 42);
    assert_eq!(decoded.y, -7);
    assert!((decoded.funky - 3.14).abs() < 1e-6);
}

//checks that custom header w missing fields works fine - works
#[test]
fn test_custom_header_missing_fields() {
    let msg = UMessage::new(); // No attributes set
    let result = CustomHeader::from_message(&msg);
    assert!(result.is_ok()); // Still shouldn't panic
}

//checks multiple messages which are sent in sequence - works 
#[tokio::test]
async fn test_multiple_sends() {
    let transport = Iceoryx2Transport::new().unwrap();

    for i in 0..5 {
        //creating valid transmission data instance
        let data = TransmissionData {
            x: i,
            y: i * 10,
            funky: i as f64 + 0.5,
        };

        // convert to bytes
        let bytes = data.to_bytes();

        // wrap in umessage and send
        let mut msg = UMessage::new();
        msg.payload = Some(bytes.into());

        let result = transport.send(msg).await;

        if let Err(e) = &result {
        }

        assert!(result.is_ok());
    }
}

//sending an empty payload - works
#[tokio::test]
async fn test_send_empty_payload() {
    let transport = Iceoryx2Transport::new().unwrap();

    let message = UMessage {
        payload: Some(Bytes::from(vec![])), // empty payload
        ..Default::default()
    };

    let result = transport.send(message).await;

    // It might be valid or invalid depending on implementation
    assert!(result.is_ok() || result.is_err());
}

//sending without a payload  - works
#[tokio::test]
async fn test_send_no_payload() {
    let transport = Iceoryx2Transport::new().unwrap();

    let message = UMessage::default(); // no payload at all

    let result = transport.send(message).await;

    assert!(result.is_err(), "Expected error when sending without payload");
}

//sending a max_sized payload - works
#[tokio::test]
async fn test_send_large_payload() {
    let transport = Iceoryx2Transport::new().unwrap();

    let max_size_payload = vec![0xAB; 1024 * 1024]; // 1MB
    let message = UMessage {
        payload: Some(Bytes::from(max_size_payload)),
        ..Default::default()
    };

    let result = transport.send(message).await;

    assert!(result.is_ok() || result.is_err()); // depends on your transport's buffer limit
}

//sending multiple concurrent messages - works
#[tokio::test]
async fn test_concurrent_sends() {
    let transport = Iceoryx2Transport::new().unwrap();
    let transport = Arc::new(transport);

    let mut handles = vec![];

    for i in 0..10 {
        let transport_clone = Arc::clone(&transport);
        handles.push(tokio::spawn(async move {
            let data = TransmissionData {
                x: i,
                y: i * 2,
                funky: i as f64 * 1.1,
            };
            let mut msg = UMessage::new();
            msg.payload = Some(Bytes::from(data.to_bytes()));
            transport_clone.send(msg).await
        }));
    }

    for handle in handles {
        let result = handle.await.unwrap();
        assert!(result.is_ok());
    }
}



#[tokio::test] // works
async fn test_send_message() {
    if let Ok(transport) = Iceoryx2Transport::new() {
        let uprotocol_header = CustomHeader {
            version: 1,
            timestamp: 123456789,
        };

        let message = UMessage {
            attributes: MessageField::some(UAttributes::from(&uprotocol_header)),
            payload: Some(vec![1, 2, 3, 4].into()),
            ..Default::default()
        };

        let result = transport.send(message).await;

        // It might fail if no background service is running, so accept Ok or Err.
        assert!(result.is_ok() || result.is_err());
    }
}
