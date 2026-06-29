//! Admin API JSON types shared by `maxio-server` and `maxio-admin` (P2-13 / P1-22).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StatusResponse {
    pub healthz: String,
    pub readyz: String,
    pub version: String,
    pub uptime_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiskInfo {
    pub total_bytes: Option<u64>,
    pub free_bytes: Option<u64>,
    pub used_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConfigInfo {
    pub region: String,
    pub erasure_coding: bool,
    pub chunk_size: u64,
    pub parity_shards: u32,
    pub max_object_bytes: u64,
    pub min_free_disk_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InfoResponse {
    pub data_dir: String,
    pub disk: DiskInfo,
    pub bucket_count: u64,
    pub object_count: u64,
    pub config: ConfigInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DoctorCheck {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DoctorResponse {
    pub ok: bool,
    pub checks: Vec<DoctorCheck>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_response_round_trips() {
        let body = StatusResponse {
            healthz: "ok".into(),
            readyz: "ok".into(),
            version: "0.0.0".into(),
            uptime_secs: 1,
        };
        let json = serde_json::to_string(&body).unwrap();
        assert!(json.contains("\"version\":\"0.0.0\""));
        let back: StatusResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back, body);
    }

    #[test]
    fn doctor_response_round_trips() {
        let body = DoctorResponse {
            ok: true,
            checks: vec![DoctorCheck {
                name: "readiness".into(),
                ok: true,
                detail: "ok".into(),
            }],
        };
        let json = serde_json::to_string(&body).unwrap();
        let back: DoctorResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back, body);
    }
}
