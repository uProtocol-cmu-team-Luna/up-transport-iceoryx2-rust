use iceoryx2::prelude::*;
use up_rust::{UMessage, UStatus, UCode, UAttributes};

#[derive(Default, Debug, ZeroCopySend)]
#[type_name("CustomHeader")]
#[repr(C)]
pub struct CustomHeader {
    pub version: i32,
    pub timestamp: u64,
}

impl CustomHeader {
    pub fn from_user_header(header: &Self) -> Result<Self, UStatus> {
        Ok(Self {
            version: header.version,
            timestamp: header.timestamp,
        })
    }
    
    pub fn from_message(_message: &UMessage) -> Result<Self, UStatus> {
        // For now, just default
        Ok(Self::default())
    }
}

// Assuming UAttributes has a field called `fields` which is Vec<(String, String)>
// Adjust if your actual UAttributes is different
impl From<&CustomHeader> for UAttributes {
    fn from(_header: &CustomHeader) -> UAttributes {
        let attrs = UAttributes::default();
        
        // Hypothetical code: add key-value pairs to attrs
        // Replace this with your real API to insert attributes
        // For example, if UAttributes has a `fields` Vec<(String, String)>:
        // attrs.fields.push(("version".to_string(), header.version.to_string()));
        // attrs.fields.push(("timestamp".to_string(), header.timestamp.to_string()));
        
        // If no such field exists, just return default or implement accordingly
        
        attrs
    }
}

#[cfg(test)]
mod tests{
use super::*;
#[test]
fn test_custom_header_from_user_header() {
    let header = CustomHeader { version: 2, timestamp: 1000 };
    let new_header = CustomHeader::from_user_header(&header).unwrap();
    assert_eq!(new_header.version, 2);
    assert_eq!(new_header.timestamp, 1000);
}
}