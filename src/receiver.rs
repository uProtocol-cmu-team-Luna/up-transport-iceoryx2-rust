use super::*;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use tokio::sync::Notify;
use log::info;

use up_rust::{
    MockUListener,UAttributes, UCode, UListener, UMessage, UMessageBuilder, UPayloadFormat,
    UStatus, UTransport, UUri,
};
use std::str::FromStr;
use iceoryx2::prelude::*;
use env_logger;

use std::sync::Once;

static INIT_LOGGER: Once = Once::new();

fn init_logger() {
    INIT_LOGGER.call_once(|| {
        env_logger::init();
    });
}

const MESSAGE_DATA: &str = "Hello World!";

pub struct Receiver{
    expected: UMessage,
    notify: Arc<Notify>,
}

impl Receiver{
    pub fn new(expected: UMessage, notify: Arc<Notify>) -> Self {
        Self { expected, notify }
    }
}

#[async_trait]
impl UListener for Receiver{
    async fn on_receive(&self, message: UMessage) {
        if let Some(payload) = &message.payload {
            println!("Received Message ID: {:#?}", message.id());
            if let (Some(expected_payload), Some(actual_payload)) = (&self.expected.payload, &message.payload) {
                assert_eq!(expected_payload, actual_payload);
            } else {
                panic!("Missing payloads in either expected or actual message");
            }
            self.notify.notify_one();
        }
    }
}

async fn register_listener_and_send(
    authority: &str,
    umessage: UMessage,
    source_filter: &UUri,
    sink_filter: Option<&UUri>,
) -> Result<(), Box<dyn std::error::Error>> {
    init_logger();
    let source_uri = UUri::try_from_parts(authority, 0xABC, 1, 0)?;
    let transport = Iceoryx2Transport::new().unwrap();
    let notify = Arc::new(Notify::new());
    let receiver = Arc::new(Receiver::new(umessage.clone(), notify.clone()));
    
    transport
        .register_listener(source_filter, sink_filter, receiver)
        .await.unwrap();

    // Send UMessage
    info!(
        "sending message: [id: {}, type: {}]",
        umessage.id_unchecked().to_hyphenated_string(),
        umessage.type_unchecked().to_cloudevent_type()
    );
    transport.send(umessage).await?;
    Ok(
        tokio::time::timeout(Duration::from_secs(3), notify.notified())
            .await
            .map_err(|_| {
                UStatus::fail_with_code(UCode::DEADLINE_EXCEEDED, "did not receive message in time")
            })?,
    )
}

#[test_case::test_case("vehicle1", 12_000, "//vehicle1/10A10B/1/CA5D", "//vehicle1/10A10B/1/CA5D"; "specific source filter")]
#[test_case::test_case("vehicle1", 0, "/D5A/3/9999", "//vehicle1/D5A/3/FFFF"; "source filter with wildcard resource ID")]
#[test_case::test_case("vehicle1", 12_000, "//vehicle1/70222/2/8001", "//*/FFFF0222/2/8001"; "source filter with wildcard authority and service instance ID")]
#[tokio::test(flavor = "multi_thread")]
async fn test_publish_gets_to_listener(
    authority: &str,
    ttl: u32,
    topic_uri: &str,
    source_filter_uri: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let topic = UUri::from_str(topic_uri)?;
    let source_filter = topic.clone();
    let payload = vec![1, 2, 3, 4]; // ✅ Raw byte array instead of TransmissionData

    let umessage = UMessageBuilder::publish(topic.clone())
        .with_priority(up_rust::UPriority::UPRIORITY_CS5)
        .with_traceparent("traceparent")
        .with_ttl(ttl)
        .build_with_payload(payload, UPayloadFormat::UPAYLOAD_FORMAT_RAW)?;

    register_listener_and_send(authority, umessage, &source_filter, None).await
}

#[tokio::test(flavor = "multi_thread")]
async fn test_unregister_listener_stops_processing_of_messages() {
    let transport =  Iceoryx2Transport::new().unwrap();
    let message_received = Arc::new(Notify::new());
    let message_received_barrier = message_received.clone();
    let mut listener = MockUListener::new();
    listener.expect_on_receive().returning(move |_msg| {
        message_received.notify_one();
    });

    let listener_to_register = Arc::new(listener);
    let msg =
        UMessageBuilder::publish(UUri::from_str("//vehicle/123/1/9000").expect("invalid topic"))
            .build()
            .expect("failed to create message");

    // [utest->dsn~utransport-registerlistener-start-invoking-listeners~1]
    assert!(transport
        .register_listener(&UUri::any(), None, listener_to_register.clone())
        .await
        .is_ok());

    // first message is expected to be processed by listener
    assert!(transport.send(msg.clone()).await.is_ok());
    assert!(
        tokio::time::timeout(Duration::from_secs(3), message_received_barrier.notified())
            .await
            .is_ok()
    );

    // [utest->dsn~utransport-unregisterlistener-stop-invoking-listeners~1]
    // after unregistering the listener,
    assert!(transport
        .unregister_listener(&UUri::any(), None, listener_to_register)
        .await
        .is_ok());
    //  no further messages should be processed
    assert!(
        tokio::time::timeout(Duration::from_secs(3), message_received_barrier.notified())
            .await
            .is_err(),
        "Expected no further messages to be processed after unregistering the listener"
    );
}

