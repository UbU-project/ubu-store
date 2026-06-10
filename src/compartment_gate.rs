use ubu_core::CompartmentLabel;

use crate::errors::Result;

pub fn validate_compartment_label(label: &str) -> Result<CompartmentLabel> {
    Ok(CompartmentLabel::parse(label.to_owned())?)
}
