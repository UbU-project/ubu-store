//! Candidate-state access for review surfaces, separate from admitted-state queries.
//! Compartment-scoped filtering is deliberately deferred by P1B-5.
pub use crate::candidates::{
    find_suppression_record, get_advisory_candidate, list_candidate_decision_events, review_queue,
};
pub use crate::models::candidate_record::{CandidateDecisionEvent, CandidateRecord};
