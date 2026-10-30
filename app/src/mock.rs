extern crate std;

use crate::{AdcReport, Board, EdgeInput, Post, Pressed, Sampler, Timer};
use core::convert::Infallible;
use embedded_hal::digital::{ErrorType, OutputPin, PinState};
use std::vec::Vec;

pub struct MockBoard;

impl Board for MockBoard {
    type Led = MockLed;
    type BlinkTimer = MockTimer;
    type Button = MockPin;
    type DebounceTimer = MockTimer;
    type Presses = MockPresses;
    type Sampler = MockSampler;
    type Reports = MockReports;
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
pub struct MockPresses(pub usize);

impl Post<Pressed> for MockPresses {
    fn posted(self, _: Pressed) -> Self {
        Self(self.0 + 1)
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct MockReports(Vec<AdcReport>);

impl From<&[AdcReport]> for MockReports {
    fn from(reports: &[AdcReport]) -> Self {
        Self(reports.to_vec())
    }
}

impl Post<AdcReport> for MockReports {
    fn posted(self, report: AdcReport) -> Self {
        Self(self.0.into_iter().chain([report]).collect())
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum MockSampler {
    Started,
    Stopped,
}

impl Sampler for MockSampler {
    type Samples = Vec<u16>;

    fn started(self) -> Self {
        Self::Started
    }

    fn stopped(self) -> Self {
        Self::Stopped
    }
}
