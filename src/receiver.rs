use super::*;
use bytes::Bytes;
use std::sync::Arc;

use async_trait::async_trait;
use up_rust::UAttributes;
use std::sync::Arc;
use up_rust::{UListener, UMessage, UStatus, UTransport, UUri, UCode};
use iceoryx2::prelude::*;
use tokio::sync::mpsc::UnboundedSender;

const MESSAGE_DATA: &str = "Hello World!";

pub struct Receiver{
    expected: UMessage,
    notify: Arc<Notify>,
}

impl Receiver{
    pub fn new(expected: UMessage,notify: Arc<Notify>) -> Self{
        Self{ expected,notify
        }
    }
}

#[async_trait]
impl UListener for Receiver{
    async fn on_receive(&self,message:UMessage)->UStatus{
        if let Some(payload)=message.payload(){
            println!("Receieved Message ID: {}", message.id());
            assert_eq!(self.expected, message);
            self.notify.notify_one();
        }

        UStatus::Ok
    }
}

async fn register_listener_and_send(
    authority: &str,
    umessage: UMessage,
    source_filter: &UUri,
    sink_filter: Option<&UUri>,
) -> Result<(), Box<dyn std::error::Error>> {
    let source_uri= UUri::try_from_parts(authority, 0xABC, 1, 0)?;
    let transport = Iceoryx2Transport::new().unwrap();
    let notify = Arc::new(Notify::new());
    let receiver=Arc::new(Receiver::new(umessage.clone(),notify.clone()));
    
    // [itest->dsn~supported-message-delivery-methods~1]
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
// [utest->dsn~up-attributes-ttl~1]
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
    let source_filter = UUri::from_str(source_filter_uri)?;
    let umessage = UMessageBuilder::publish(topic.clone())
        .with_priority(up_rust::UPriority::UPRIORITY_CS5)
        .with_traceparent("traceparent")
        .with_ttl(ttl)
        .build_with_payload(MESSAGE_DATA, UPayloadFormat::UPAYLOAD_FORMAT_TEXT)?;

    register_listener_and_send(authority, umessage, &source_filter, None).await
}



