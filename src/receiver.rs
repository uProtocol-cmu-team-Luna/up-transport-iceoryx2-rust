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
impl UListener for Receiver{
    async fn on_receive(&self,message:UMessage)->UStatus{
        if let Some(payload)=message.payload(){
            println!("Receieved Message ID: {}", message.id());
        }

        UStatus::Ok
    }
}

#[tokio::test]
async fn test_receiver(){
    let receiver= Receiver::new();
    let transport = Iceoryx2Transport::new().unwrap();
    source_URI= UUri
    sink_filter=
    transport.register_listener(receiver).await.unwrap();

}


