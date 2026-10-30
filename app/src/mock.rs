extern crate std;

use crate::{Board, EdgeInput, Event, Post, Timer};
use core::convert::Infallible;
use embedded_hal::digital::{ErrorType, OutputPin, PinState};
use std::vec::Vec;

pub struct MockBoard;

impl Board for MockBoard {
    type Led = MockLed;
    type BlinkTimer = MockTimer;
    type Button = MockPin;
    type DebounceTimer = MockTimer;
    type Events = MockEvents;
}

#[derive(Debug, PartialEq, Eq)]
pub struct MockLed(pub PinState);

impl ErrorType for MockLed {
    type Error = Infallible;
}

impl OutputPin for MockLed {
    fn set_low(&mut self) -> Result<(), Infallible> {
        self.0 = PinState::Low;
        Ok(())
    }

    fn set_high(&mut self) -> Result<(), Infallible> {
        self.0 = PinState::High;
        Ok(())
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum MockTimer {
    Started,
    Stopped,
}

impl Timer for MockTimer {
    fn started(self) -> Self {
        Self::Started
    }

    fn stopped(self) -> Self {
        Self::Stopped
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct MockPin {
    level: PinState,
    armed: bool,
}

impl MockPin {
    pub const fn armed(level: PinState) -> Self {
        Self { level, armed: true }
    }

    pub const fn disarmed(level: PinState) -> Self {
        Self {
            level,
            armed: false,
        }
    }

    pub const fn at(self, level: PinState) -> Self {
        Self { level, ..self }
    }
}

impl EdgeInput for MockPin {
    fn armed(self) -> Self {
        Self::armed(self.level)
    }

    fn disarmed(self) -> Self {
        Self::disarmed(self.level)
    }

    fn sampled(self) -> (PinState, Self) {
        (self.level, self)
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct MockEvents(Vec<Event>);

impl From<&[Event]> for MockEvents {
    fn from(events: &[Event]) -> Self {
        Self(events.to_vec())
    }
}

impl Post<Event> for MockEvents {
    fn posted(self, event: Event) -> Self {
        Self(self.0.into_iter().chain([event]).collect())
    }
}
