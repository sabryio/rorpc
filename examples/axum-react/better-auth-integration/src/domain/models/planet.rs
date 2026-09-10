use rorpc::ZodTs;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, ZodTs)]
pub struct Planet {
    pub id: i32,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct FindPlanetInput {
    pub id: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub q: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, ZodTs)]
pub struct FindPlanetQuery {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub q: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, ZodTs)]
pub struct DeletePlanetInput {
    pub id: i32,
}

#[derive(Debug, Deserialize, Serialize, ZodTs)]
pub struct CreatePlanetInput {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, ZodTs)]
pub struct ListPlanetsPaginatedInput {
    pub limit: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<usize>,
}

#[derive(Debug, Serialize, ZodTs)]
pub struct ListPlanetsPaginatedOutput {
    pub items: Vec<Planet>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_param: Option<usize>,
}

#[derive(Debug, Serialize, ZodTs)]
pub struct EventData {
    pub message: String,
    pub count: u32,
}
