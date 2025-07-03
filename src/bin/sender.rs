use up_transport_iceoryx2_rust::{Iceoryx2Transport, TransmissionData};
use up_rust::{UTransport, UMessageBuilder, UPayloadFormat, UUri};

#[tokio::main]
async fn main() {
    println!("Starting sender application...");

    let transport = match Iceoryx2Transport::new() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Failed to create transport: {:?}", e);
            return;
        }
    };

    // Define a topic URI to publish to
    let source_uri = UUri::try_from_parts("vehicle", 0x1000, 1, 0x8001).unwrap();

    // Create a sample payload
    let data = TransmissionData {
        x: 123,
        y: 456,
        funky: 9.87,
    };
    let payload_bytes = data.to_bytes();

    // Build the UMessage
    let msg = UMessageBuilder::publish(source_uri.clone())
        .build_with_payload(payload_bytes, UPayloadFormat::UPAYLOAD_FORMAT_RAW)
        .expect("Failed to build message");

    println!("Sending message to URI: {}", source_uri);

    // Send the message
    match transport.send(msg).await {
        Ok(_) => println!("Message sent successfully!"),
        Err(e) => eprintln!("Failed to send message: {:?}", e),
    }
}
