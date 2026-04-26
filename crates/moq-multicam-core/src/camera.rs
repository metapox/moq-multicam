use serde::{Deserialize, Serialize};

/// Configuration for a single camera in a multi-camera setup.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CameraConfig {
    /// Camera name used in track paths (e.g. "front", "rear").
    pub name: String,
    /// Lower value = higher priority. 0 is highest.
    pub priority: u8,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_roundtrip() {
        let config = CameraConfig {
            name: "front".into(),
            priority: 0,
        };
        let json = serde_json::to_string(&config).unwrap();
        let parsed: CameraConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name, "front");
        assert_eq!(parsed.priority, 0);
    }
}
