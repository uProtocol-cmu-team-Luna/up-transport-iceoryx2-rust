use up_rust::{UMessageBuilder, UPayloadFormat, UUri};
use env_logger;
use std::sync::Once;
use std::str::FromStr;
use up_transport_iceoryx2_rust::{Iceoryx2Transport, TransmissionData};
use up_rust::UTransport;


static INIT_LOGGER: Once = Once::new();

fn init_logger() {
    INIT_LOGGER.call_once(|| {
        env_logger::init();
    });
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_logger();

    let topic = UUri::from_str("//vehicle_shared/10A10B/1/CA5D")?;
    let transport = Iceoryx2Transport::new().unwrap();

    loop {
        let data = TransmissionData { x: 1, y: 2, funky: 3.14 };
        let payload = data.to_bytes();

        let umessage = UMessageBuilder::publish(topic.clone())
            .build_with_payload(payload, UPayloadFormat::UPAYLOAD_FORMAT_RAW)?;

        transport.send(umessage).await?;

        println!("Message sent.");

        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}
