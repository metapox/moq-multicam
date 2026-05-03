use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum TrackPathError {
    #[error("invalid track path: {0}")]
    Invalid(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Quality {
    High,
    Low,
}

impl Quality {
    pub fn track_name(self) -> &'static str {
        match self {
            Quality::High => "video",
            Quality::Low => "video-low",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TrackKind {
    Camera,
    Meta,
}

/// Structured representation of a moq-multicam track path.
///
/// New design: one Broadcast per camera.
///   Camera broadcast: `vehicle/{vehicle_id}/camera/{camera_name}`
///     Tracks: `video`, `video-low`
///   Meta broadcast: `vehicle/{vehicle_id}/meta`
///     Tracks: `manifest`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackPath {
    pub vehicle_id: String,
    pub kind: TrackKind,
    pub name: String,
    pub quality: Option<Quality>,
}

impl TrackPath {
    pub fn camera(vehicle_id: &str, camera_name: &str, quality: Quality) -> Self {
        Self {
            vehicle_id: vehicle_id.into(),
            kind: TrackKind::Camera,
            name: camera_name.into(),
            quality: Some(quality),
        }
    }

    pub fn meta(vehicle_id: &str, name: &str) -> Self {
        Self {
            vehicle_id: vehicle_id.into(),
            kind: TrackKind::Meta,
            name: name.into(),
            quality: None,
        }
    }

    /// Broadcast path for this track (used with Origin::publish_broadcast).
    pub fn broadcast_path(&self) -> String {
        match self.kind {
            TrackKind::Camera => format!("vehicle/{}/camera/{}", self.vehicle_id, self.name),
            TrackKind::Meta => format!("vehicle/{}/meta", self.vehicle_id),
        }
    }

    /// Track name within the broadcast.
    pub fn track_name(&self) -> String {
        match self.kind {
            TrackKind::Camera => self
                .quality
                .unwrap_or(Quality::High)
                .track_name()
                .to_string(),
            TrackKind::Meta => self.name.clone(),
        }
    }

    pub fn parse(path: &str) -> Result<Self, TrackPathError> {
        let parts: Vec<&str> = path.split('/').collect();
        let err = || TrackPathError::Invalid(path.into());

        if parts.len() < 4 || parts[0] != "vehicle" {
            return Err(err());
        }

        let vehicle_id = parts[1];

        match parts[2] {
            "camera" if parts.len() == 5 => {
                let camera_name = parts[3];
                let quality = match parts[4] {
                    "video" => Quality::High,
                    "video-low" => Quality::Low,
                    _ => return Err(err()),
                };
                Ok(Self::camera(vehicle_id, camera_name, quality))
            }
            "meta" if parts.len() == 4 => Ok(Self::meta(vehicle_id, parts[3])),
            _ => Err(err()),
        }
    }
}

impl fmt::Display for TrackPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.broadcast_path(), self.track_name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camera_high_roundtrip() {
        let path = TrackPath::camera("truck-01", "front", Quality::High);
        assert_eq!(path.broadcast_path(), "vehicle/truck-01/camera/front");
        assert_eq!(path.track_name(), "video");
        assert_eq!(path.to_string(), "vehicle/truck-01/camera/front/video");

        let parsed = TrackPath::parse(&path.to_string()).unwrap();
        assert_eq!(parsed, path);
    }

    #[test]
    fn camera_low_roundtrip() {
        let path = TrackPath::camera("truck-01", "rear", Quality::Low);
        assert_eq!(path.broadcast_path(), "vehicle/truck-01/camera/rear");
        assert_eq!(path.track_name(), "video-low");
        assert_eq!(path.to_string(), "vehicle/truck-01/camera/rear/video-low");

        let parsed = TrackPath::parse(&path.to_string()).unwrap();
        assert_eq!(parsed, path);
    }

    #[test]
    fn meta_roundtrip() {
        let path = TrackPath::meta("truck-01", "manifest");
        assert_eq!(path.broadcast_path(), "vehicle/truck-01/meta");
        assert_eq!(path.track_name(), "manifest");
        assert_eq!(path.to_string(), "vehicle/truck-01/meta/manifest");

        let parsed = TrackPath::parse(&path.to_string()).unwrap();
        assert_eq!(parsed, path);
    }

    #[test]
    fn parse_invalid() {
        assert!(TrackPath::parse("").is_err());
        assert!(TrackPath::parse("vehicle").is_err());
        assert!(TrackPath::parse("vehicle/truck-01/camera/front/audio").is_err());
        assert!(TrackPath::parse("something/else").is_err());
    }
}
