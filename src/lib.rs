use async_trait::async_trait;
use iceoryx2::prelude::*;
use std::sync::Arc;
use up_rust::UAttributes;
use up_rust::{UCode, UListener, UMessage, UStatus, UTransport, UUri};

mod custom_header;
mod transmission_data;
pub use custom_header::CustomHeader;
pub use transmission_data::TransmissionData;

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::thread;

#[derive(Debug)]
enum TransportCommand {
    Send {
        message: UMessage,
        response: std::sync::mpsc::Sender<Result<(), UStatus>>,
    },
    RegisterListener {
        source_filter: UUri,
        sink_filter: Option<UUri>,
        response: std::sync::mpsc::Sender<Result<(), UStatus>>,
    },
    UnregisterListener {
        source_filter: UUri,
        sink_filter: Option<UUri>,
        response: std::sync::mpsc::Sender<Result<(), UStatus>>,
    },
}

pub struct Iceoryx2Transport {
    command_sender: std::sync::mpsc::Sender<TransportCommand>,
}

impl Iceoryx2Transport {
    pub fn new() -> Result<Self, UStatus> {
        let (tx, rx) = std::sync::mpsc::channel();

        thread::spawn(move || {
            Self::background_task(rx);
        });

        Ok(Self { command_sender: tx })
    }

    fn encode_uuri_segments(uuri: &UUri) -> Vec<String> {
        vec![
            uuri.authority_name.clone(), // e.g., "device1"
            Self::encode_hex_no_leading_zeros(uuri.uentity_type_id() as u32),
            Self::encode_hex_no_leading_zeros(uuri.uentity_instance_id() as u32),
            Self::encode_hex_no_leading_zeros(uuri.uentity_major_version() as u32),
            Self::encode_hex_no_leading_zeros(uuri.resource_id() as u32),
        ]
    }
    
    fn encode_hex_no_leading_zeros(value: u32) -> String {
        format!("{:X}", value)
    }
    
    // returns a correct iceoryx2 service name based on the UMessage type
    // and its source/sink URIs.
    fn compute_service_name(message: &UMessage) -> Result<String, UStatus> {
        let join_segments = |segments: Vec<String>| segments.join("/");
    
        if message.is_publish() {
            let source = message.source().ok_or_else(|| {
                UStatus::fail_with_code(UCode::INVALID_ARGUMENT, "Missing source URI")
            })?;
            let segments = Self::encode_uuri_segments(source);
            Ok(format!("up/{}", join_segments(segments)))
        } else if message.is_request() {
            let sink = message.sink().ok_or_else(|| {
                UStatus::fail_with_code(UCode::INVALID_ARGUMENT, "Missing sink URI")
            })?;
            let segments = Self::encode_uuri_segments(sink);
            Ok(format!("up/{}", join_segments(segments)))
        } else if message.is_response() || message.is_notification() {
            let source = message.source().ok_or_else(|| {
                UStatus::fail_with_code(UCode::INVALID_ARGUMENT, "Missing source URI")
            })?;
            let sink = message.sink().ok_or_else(|| {
                UStatus::fail_with_code(UCode::INVALID_ARGUMENT, "Missing sink URI")
            })?;
    
            let source_segments = Self::encode_uuri_segments(source);
            let sink_segments = Self::encode_uuri_segments(sink);
            Ok(format!(
                "up/{}/{}",
                join_segments(source_segments),
                join_segments(sink_segments)
            ))
        } else {
            Err(UStatus::fail_with_code(
                UCode::INVALID_ARGUMENT,
                "Unsupported UMessageType",
            ))
        }
    }           

    fn background_task(rx: std::sync::mpsc::Receiver<TransportCommand>) {
        let node = match NodeBuilder::new().create::<ipc::Service>() {
            Ok(node) => node,
            Err(e) => {
                eprintln!("Failed to create iceoryx2 node: {}", e);
                return;
            }
        };
    
        let mut publishers: HashMap<
            String,
            iceoryx2::port::publisher::Publisher<ipc::Service, TransmissionData, CustomHeader>,
        > = HashMap::new();
    
        while let Ok(command) = rx.recv() {
            match command {
                TransportCommand::Send { message, response } => {
                    let service_name = match Self::compute_service_name(&message) {
                        Ok(name) => name,
                        Err(e) => {
                            let _ = response.send(Err(e));
                            continue;
                        }
                    };
    
                    let publisher = publishers.entry(service_name.clone()).or_insert_with(|| {
                        let service = node
                            .service_builder(&service_name.as_str().try_into().unwrap())
                            .publish_subscribe::<TransmissionData>()
                            .user_header::<CustomHeader>()
                            .open_or_create()
                            .expect("Failed to create service");
    
                        service
                            .publisher_builder()
                            .create()
                            .expect("Failed to create publisher")
                    });
    
                    let result = Self::handle_send(publisher, message);
                    let _ = response.send(result);
                }
                TransportCommand::RegisterListener { response, .. } => {
                    let _ = response.send(Ok(())); // TODO
                }
                TransportCommand::UnregisterListener { response, .. } => {
                    let _ = response.send(Ok(())); // TODO
                }
            }
        }
    }  

    fn handle_send(
        publisher: &iceoryx2::port::publisher::Publisher<ipc::Service, TransmissionData, CustomHeader>,
        message: UMessage
    ) -> Result<(), UStatus> {
        let transmission_data = TransmissionData::from_message(&message)?;
        let header = CustomHeader::from_message(&message)?;
    
        let sample = publisher.loan_uninit()
            .map_err(|e| UStatus::fail_with_code(UCode::INTERNAL, &format!("Failed to loan sample: {e}")))?;
    
        let mut sample_final = sample.write_payload(transmission_data);
        *sample_final.user_header_mut() = header;
    
        sample_final.send()
            .map_err(|e| UStatus::fail_with_code(UCode::INTERNAL, &format!("Failed to send: {e}")))?;
    
        Ok(())
    }    
}

#[async_trait]
impl UTransport for Iceoryx2Transport {
    async fn send(&self, message: UMessage) -> Result<(), UStatus> {
        let (tx, rx) = std::sync::mpsc::channel();

        let command = TransportCommand::Send {
            message,
            response: tx,
        };

        self.command_sender
            .send(command)
            .map_err(|_| UStatus::fail_with_code(UCode::INTERNAL, "Background task has died"))?;

        rx.recv().map_err(|_| {
            UStatus::fail_with_code(UCode::INTERNAL, "Background task response failed")
        })?
    }

    async fn register_listener(
        &self,
        source_filter: &UUri,
        sink_filter: Option<&UUri>,
        listener: Arc<dyn UListener>,
    ) -> Result<(), UStatus> {
        let (tx, rx) = std::sync::mpsc::channel();

        let command = TransportCommand::RegisterListener {
            source_filter: source_filter.clone(),
            sink_filter: sink_filter.cloned(),
            response: tx,
        };

        self.command_sender
            .send(command)
            .map_err(|_| UStatus::fail_with_code(UCode::INTERNAL, "Background task has died"))?;

        rx.recv().map_err(|_| {
            UStatus::fail_with_code(UCode::INTERNAL, "Background task response failed")
        })?
    }

    async fn unregister_listener(
        &self,
        source_filter: &UUri,
        sink_filter: Option<&UUri>,
        listener: Arc<dyn UListener>,
    ) -> Result<(), UStatus> {
        let (tx, rx) = std::sync::mpsc::channel();

        let command = TransportCommand::UnregisterListener {
            source_filter: source_filter.clone(),
            sink_filter: sink_filter.cloned(),
            response: tx,
        };

        self.command_sender
            .send(command)
            .map_err(|_| UStatus::fail_with_code(UCode::INTERNAL, "Background task has died"))?;

        rx.recv().map_err(|_| {
            UStatus::fail_with_code(UCode::INTERNAL, "Background task response failed")
        })?
    }
}

fn test_uri(authority: &str, instance: u16, typ: u16, version: u8, resource: u16) -> UUri {
    let entity_id = ((instance as u32) << 16) | (typ as u32);
    UUri::try_from_parts(authority, entity_id, version, resource).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use up_rust::{UPayloadFormat, UStatus, UMessageBuilder};

    use up_rust::UUID;
    use std::convert::TryFrom;

    // fn dummy_uuid() -> up_rust::UUID {
    //     // let uuid = UUID::new();
    //     let uuid = up_rust::UUID::new(); 
    //     up_rust::UUID::try_from(uuid).expect("Valid UUID conversion")
    // }
    fn dummy_uuid() -> up_rust::UUID {
        up_rust::UUID::build()
    }    
   
    // Helper function to create a test URI
    fn test_uri(authority: &str, instance: u16, typ: u16, version: u8, resource: u16) -> UUri {
        let entity_id = ((instance as u32) << 16) | (typ as u32);
        UUri::try_from_parts(authority, entity_id, version, resource).unwrap()
    }

    #[test]
    fn test_publish_service_name() {
        let source = test_uri("device1", 0x0000, 0x10AB, 0x03, 0x80CD);
        let message = UMessageBuilder::publish(source.clone())
            .build_with_payload(vec![], UPayloadFormat::UPAYLOAD_FORMAT_RAW)
            .unwrap();

        let name = Iceoryx2Transport::compute_service_name(&message).unwrap();
        assert_eq!(name, "up/device1/10AB/0/3/80CD");
    }

    #[test]
    fn test_notification_service_name() {
        let source = test_uri("device1", 0x0000, 0x10AB, 0x03, 0x80CD);
        let sink = test_uri("device1", 0x0000, 0x30EF, 0x04, 0x0000);
        let message = UMessageBuilder::notification(source.clone(), sink.clone())
            .build_with_payload(vec![], UPayloadFormat::UPAYLOAD_FORMAT_RAW)
            .unwrap();

        let name = Iceoryx2Transport::compute_service_name(&message).unwrap();
        assert_eq!(name, "up/device1/10AB/0/3/80CD/device1/30EF/0/4/0");
    }

    #[test]
    fn test_rpc_request_service_name() {
        let sink = test_uri("device1", 0x0000, 0x00CD, 0x04, 0x000B);
        let reply_to = test_uri("device1", 0x0000, 0x0001, 0x01, 0x0000); // Dummy reply URI

        let message = UMessageBuilder::request(sink.clone(), reply_to, 1000)
            .build_with_payload(vec![], UPayloadFormat::UPAYLOAD_FORMAT_RAW)
            .unwrap();

        let name = Iceoryx2Transport::compute_service_name(&message).unwrap();
        assert_eq!(name, "up/device1/CD/0/4/B");
    }


    #[test]
    fn test_rpc_response_service_name() {
        let source = test_uri("device1", 0x0001, 0x0020, 0x01, 0x0000);
        let sink = test_uri("device1", 0x0001, 0x00CD, 0x04, 0x1000);
        let uuid = dummy_uuid();

        let message = UMessageBuilder::response(source.clone(), uuid, sink.clone())
            .build_with_payload(vec![], UPayloadFormat::UPAYLOAD_FORMAT_RAW)
            .unwrap();

        let name = Iceoryx2Transport::compute_service_name(&message).unwrap();

        assert_eq!(
            name,
            "up/device1/CD/1/4/1000/device1/20/1/1/0"
        );
    }



    #[test]
    fn test_missing_uri_error() {
        let message = UMessage::new(); // no source or sink
        let result = Iceoryx2Transport::compute_service_name(&message);

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().get_code(), UCode::INVALID_ARGUMENT);
    }
}
