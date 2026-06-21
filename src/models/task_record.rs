use serde::{Deserialize, Serialize};

use crate::errors::{Result, StoreError};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum TaskDurationEstimate {
    Fixed {
        seconds: u64,
    },
    ShiftedLognormalP95 {
        min_seconds: u64,
        mode_seconds: u64,
        p95_seconds: u64,
    },
}

impl TaskDurationEstimate {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Fixed { seconds } if *seconds == 0 => Err(invalid_task_field(
                "duration_estimate.fixed.seconds must be greater than zero",
            )),
            Self::ShiftedLognormalP95 {
                min_seconds,
                mode_seconds,
                p95_seconds,
            } if !(min_seconds < mode_seconds && mode_seconds < p95_seconds) => {
                Err(invalid_task_field(
                    "duration_estimate must satisfy min_seconds < mode_seconds < p95_seconds",
                ))
            }
            _ => Ok(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskCorrelationGroup {
    pub group: String,
    pub strength: f64,
}

fn invalid_task_field(message: impl Into<String>) -> StoreError {
    StoreError::InvalidPayload(format!("core/task schema: {}", message.into()))
}
