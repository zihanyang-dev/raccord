#![forbid(unsafe_code)]

use raccord_time::SampleIndex;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct AudioBusId(String);

impl AudioBusId {
    pub fn new(value: impl Into<String>) -> Option<Self> {
        let value = value.into();
        (!value.is_empty()).then_some(Self(value))
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AudioGraph {
    buses: Vec<AudioBusId>,
}

impl AudioGraph {
    pub fn add_bus(&mut self, id: AudioBusId) {
        self.buses.push(id);
    }

    pub fn buses(&self) -> &[AudioBusId] {
        &self.buses
    }

    pub const fn sample_start() -> SampleIndex {
        SampleIndex::ZERO
    }
}
