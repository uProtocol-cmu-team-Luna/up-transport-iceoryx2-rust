use super::*;
use bytes::Bytes;
use std::sync::Arc;

use async_trait::async_trait;
use up_rust::UAttributes;
use std::sync::Arc;
use up_rust::{UListener, UMessage, UStatus, UTransport, UUri, UCode};
use iceoryx2::prelude::*;
use tokio::sync::mpsc::UnboundedSender;


pub struct Receiver{

}

impl Receiver{
    pub fn new() -> Self{
        Self{

        }
    }
}

#[async_trait]
impl UListener for Receiver {
    async fn on_receive(&self, message: UMessage) {
        if message.payload().is_some() {
            println!("Received Message ID: {}", message.id());
        }
    }
}

// #[tokio::test]
// async fn test_receiver() {
//     let receiver = Receiver::new();
//     let transport = Iceoryx2Transport::new().unwrap();
//     let source_uri = UUri::default();
//     let sink_filter = None;
//     transport
//         .register_listener(&source_uri, sink_filter, Arc::new(receiver))
//         .await
//         .unwrap();
// }
