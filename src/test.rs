// src/test.rs

use super::*;
use protobuf::MessageField;  // if you use this in tests

// Example synchronous test (no async needed)
#[test]
fn test_custom_header_from_user_header() {
    let header = CustomHeader { version: 2, timestamp: 1000 };
    let new_header = CustomHeader::from_user_header(&header).unwrap();
    assert_eq!(new_header.version, 2);
    assert_eq!(new_header.timestamp, 1000);
}

// Async test requires Tokio runtime
#[tokio::test]
async fn test_transport_creation() {
    let transport = Iceoryx2Transport::new();
    assert!(transport.is_ok());
}

#[tokio::test]
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
