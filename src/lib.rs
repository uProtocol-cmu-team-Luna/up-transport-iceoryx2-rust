use async_trait::async_trait;
use iceoryx2::prelude::*;
use protobuf::MessageField;
use std::sync::Arc;
use up_rust::{UAttributes, UCode, UListener, UMessage, UStatus, UTransport, UUri};

mod custom_header;
pub use custom_header::CustomHeader;

mod raw_bytes;
use raw_bytes::RawBytes;

use std::collections::HashMap;
use std::thread;

enum TransportCommand {
    Send {
        message: UMessage,
        response: std::sync::mpsc::Sender<Result<(), UStatus>>,
    },
    RegisterListener {
        source_filter: UUri,
        sink_filter: Option<UUri>,
        listener: Arc<dyn UListener>,
        response: std::sync::mpsc::Sender<Result<(), UStatus>>,
    },
    UnregisterListener {
        source_filter: UUri,
        sink_filter: Option<UUri>,
        listener: Arc<dyn UListener>,
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
        let authority = if uuri.authority_name == "*" {
            "wildcard".to_string()
        } else {
            uuri.authority_name.clone()
        };    
        vec![
            authority,
            Self::encode_hex_no_leading_zeros(uuri.uentity_type_id() as u32),
            Self::encode_hex_no_leading_zeros(uuri.uentity_instance_id() as u32),
            Self::encode_hex_no_leading_zeros(uuri.uentity_major_version() as u32),
            Self::encode_hex_no_leading_zeros(uuri.resource_id() as u32),
        ]
    }

    fn encode_hex_no_leading_zeros(value: u32) -> String {
        format!("{:X}", value)
    }

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

    fn compute_listener_service_name(
        source_filter: &UUri,
        sink_filter: Option<&UUri>,
    ) -> Result<String, UStatus> {
        let join_segments = |segments: Vec<String>| segments.join("/");

        match sink_filter {
            None => {
                let segments = Self::encode_uuri_segments(source_filter);
                Ok(format!("up/{}", join_segments(segments)))
            }
            Some(sink) => {
                let is_wildcard_entity = source_filter.uentity_type_id() == 0xFFFF
                    && source_filter.uentity_instance_id() == 0xFFFF;
                let is_wildcard_version = source_filter.uentity_major_version() == 0xFF;
                let is_wildcard_resource = source_filter.resource_id() == 0xFFFF;

                if is_wildcard_entity && is_wildcard_version && is_wildcard_resource {
                    let segments = Self::encode_uuri_segments(sink);
                    Ok(format!("up/{}", join_segments(segments)))
                } else {
                    let source_segments = Self::encode_uuri_segments(source_filter);
                    let sink_segments = Self::encode_uuri_segments(sink);
                    Ok(format!(
                        "up/{}/{}",
                        join_segments(source_segments),
                        join_segments(sink_segments)
                    ))
                }
            }
        }
    }

    fn background_task(rx: std::sync::mpsc::Receiver<TransportCommand>) {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to create Tokio runtime");

        rt.block_on(async {
            let node = match NodeBuilder::new().create::<ipc::Service>() {
                Ok(node) => node,
                Err(e) => {
                    eprintln!("Failed to create iceoryx2 node: {}", e);
                    return;
                }
            };

            let mut publishers: HashMap<
                String,
                iceoryx2::port::publisher::Publisher<ipc::Service, RawBytes, CustomHeader>,
            > = HashMap::new();

            let mut subscribers: HashMap<
                String,
                iceoryx2::port::subscriber::Subscriber<ipc::Service, RawBytes, CustomHeader>,
            > = HashMap::new();

            let mut listeners: HashMap<String, Vec<Arc<dyn UListener>>> = HashMap::new();

            loop {
                while let Ok(command) = rx.try_recv() {
                    match command {
                        TransportCommand::Send { message, response } => {
                            let service_name = match Self::compute_service_name(&message) {
                                Ok(name) => name,
                                Err(e) => {
                                    let _ = response.send(Err(e));
                                    continue;
                                }
                            };

                            let publisher =
                                publishers.entry(service_name.clone()).or_insert_with(|| {
                                    let service_name_res: Result<ServiceName, _> =
                                        service_name.as_str().try_into();
                                    let service = node
                                        .service_builder(&service_name_res.unwrap())
                                        .publish_subscribe::<RawBytes>()
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
                        TransportCommand::RegisterListener {
                            source_filter,
                            sink_filter,
                            listener,
                            response,
                        } => {
                            let res = Self::handle_register_listener(
                                &node,
                                &mut subscribers,
                                &mut listeners,
                                source_filter,
                                sink_filter.as_ref(),
                                listener,
                            );
                            let _ = response.send(res);
                        }
                        TransportCommand::UnregisterListener {
                            source_filter,
                            sink_filter,
                            listener,
                            response,
                        } => {
                            let res = Self::handle_unregister_listener(
                                &mut subscribers,
                                &mut listeners,
                                source_filter,
                                sink_filter.as_ref(),
                                &listener,
                            );
                            let _ = response.send(res);
                        }
                    }
                }

                for (service_name, subscriber) in subscribers.iter() {
                    while let Some(sample) = subscriber.receive().ok().flatten() {
                        if let Some(listeners) = listeners.get(service_name) {
                            for listener in listeners {
                                let mut new_umessage = UMessage::new();
                                new_umessage.attributes =
                                    MessageField::some(UAttributes::from(sample.user_header()));
                                new_umessage.payload =
                                    Some(sample.payload().to_bytes().into());
                                let listener_clone = listener.clone();
                                tokio::spawn(async move {
                                    listener_clone.on_receive(new_umessage).await;
                                });
                            }
                        }
                    }
                }

                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        });
    }

    fn handle_unregister_listener(
        subscribers: &mut HashMap<
            String,
            iceoryx2::port::subscriber::Subscriber<ipc::Service, RawBytes, CustomHeader>,
        >,
        listeners: &mut HashMap<String, Vec<Arc<dyn UListener>>>,
        source_filter: UUri,
        sink_filter: Option<&UUri>,
        listener: &Arc<dyn UListener>,
    ) -> Result<(), UStatus> {
        let service_name = match Self::compute_listener_service_name(&source_filter, sink_filter) {
            Ok(name) => name,
            Err(e) => {
                return Err(e);
            }
        };

        if let Some(listener_vec) = listeners.get_mut(&service_name) {
            listener_vec.retain(|l| !Arc::ptr_eq(l, listener));
            if listener_vec.is_empty() {
                listeners.remove(&service_name);
                subscribers.remove(&service_name);
            }
        }

        Ok(())
    }

    fn handle_register_listener(
        node: &Node<ipc::Service>,
        subscribers: &mut HashMap<
            String,
            iceoryx2::port::subscriber::Subscriber<ipc::Service, RawBytes, CustomHeader>,
        >,
        listeners: &mut HashMap<String, Vec<Arc<dyn UListener>>>,
        source_filter: UUri,
        sink_filter: Option<&UUri>,
        listener: Arc<dyn UListener>,
    ) -> Result<(), UStatus> {
        let service_name = match Self::compute_listener_service_name(&source_filter, sink_filter) {
            Ok(name) => name,
            Err(e) => {
                return Err(e);
            }
        };

        if !subscribers.contains_key(&service_name) {
            let service_name_res: Result<ServiceName, _> = service_name.as_str().try_into();
            let service = node
                .service_builder(&service_name_res.unwrap())
                .publish_subscribe::<RawBytes>()
                .user_header::<CustomHeader>()
                .open_or_create()
                .expect("Failed to create service");

            let subscriber = service
                .subscriber_builder()
                .create()
                .expect("Failed to create subscriber");
            subscribers.insert(service_name.clone(), subscriber);
        }

        listeners.entry(service_name).or_default().push(listener);
        Ok(())
    }

    fn handle_send(
        publisher: &iceoryx2::port::publisher::Publisher<
            ipc::Service,
            RawBytes,
            CustomHeader,
        >,
        message: UMessage,
    ) -> Result<(), UStatus> {
        let payload_bytes = message.payload.clone().unwrap_or_default().to_vec();
        let raw_payload = RawBytes::from_bytes(&payload_bytes);
        let header = CustomHeader::from_message(&message)?;

        let sample = publisher.loan_uninit().map_err(|e| {
            UStatus::fail_with_code(UCode::INTERNAL, &format!("Failed to loan sample: {e}"))
        })?;

        let mut sample_final = sample.write_payload(raw_payload);
        *sample_final.user_header_mut() = header;

        sample_final.send().map_err(|e| {
            UStatus::fail_with_code(UCode::INTERNAL, &format!("Failed to send: {e}"))
        })?;

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
            listener,
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
            listener,
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

mod receiver;

#[cfg(test)]
mod tests {
    // (keep your tests unchanged, they're correct!)
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;
    use up_rust::{UMessageBuilder, UPayloadFormat};

    fn dummy_uuid() -> up_rust::UUID {
        up_rust::UUID::build()
    }

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
        let reply_to = test_uri("device1", 0x0000, 0x0001, 0x01, 0x0000);
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
        assert_eq!(name, "up/device1/CD/1/4/1000/device1/20/1/1/0");
    }

    #[test]
    fn test_missing_uri_error() {
        let message = UMessage::new();
        let result = Iceoryx2Transport::compute_service_name(&message);

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().get_code(), UCode::INVALID_ARGUMENT);
    }
}