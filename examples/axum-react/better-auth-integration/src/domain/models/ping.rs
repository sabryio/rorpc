use rorpc::ZodTs;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, ZodTs)]
pub struct PingResponse {
    pub id: Uuid,
    pub message: String,
}
