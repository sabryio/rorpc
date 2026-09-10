use rorpc::ZodTs;

/// Server-Sent Events for real-time updates
#[derive(Debug, Clone, ZodTs, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum SseEvent {
    /// A new campaign was created and queued.
    CampaignCreated {
        campaign_id: String,
        title: String,
    },
    
    /// Campaign status changed
    CampaignStatusChanged {
        campaign_id: String,
        status: String,
    },
    
    /// Progress update for a campaign
    CampaignProgress {
        campaign_id: String,
        sent: u32,
        total: u32,
        failed: u32,
    },
}
