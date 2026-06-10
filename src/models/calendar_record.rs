use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::FromRow;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, FromRow)]
pub struct CalendarRecord {
    pub id: String,
    pub plan_id: String,
    pub window_start: String,
    pub window_end: String,
    pub payload_json: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewCalendarRecord {
    pub id: String,
    pub plan_id: String,
    pub window_start: String,
    pub window_end: String,
    pub payload: Value,
    pub created_at: String,
}
