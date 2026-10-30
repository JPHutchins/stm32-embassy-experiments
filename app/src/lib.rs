#![no_std]

mod adc;
mod blink;
mod button;
mod double_buffer;
#[cfg(test)]
mod mock;

pub use adc::{Adc, AdcEvent, AdcReport, DECIMATED_LEN, Input, Measurement, SEQUENCE};
use blink::Activity;
pub use blink::{Blink, BlinkEvent};
pub use button::{Button, ButtonEvent};
pub use double_buffer::{Completion, DoubleBuffer, Half, Lease};

use core::convert::Infallible;
use core::ops::Deref;
use embedded_hal::digital::{OutputPin, PinState};

pub trait Machine: Sized {
    type Event;

    fn run(self, event: Self::Event) -> Self;
}

pub trait Board {
    type Led: OutputPin<Error = Infallible>;
    type BlinkTimer: Timer;
    type Button: EdgeInput;
    type DebounceTimer: Timer;
    type Presses: Post<Pressed>;
    type Sampler: Sampler;
    type Reports: Post<AdcReport>;
}

pub type Samples<B> = <<B as Board>::Sampler as Sampler>::Samples;

pub struct App<B: Board> {
    blink: Blink<B::Led, B::BlinkTimer>,
    button: Button<B::Button, B::DebounceTimer, B::Presses>,
    adc: Adc<B::Sampler, B::Reports>,
}

#[derive(Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Event<S> {
    Blink(BlinkEvent),
    Button(ButtonEvent),
    Adc(AdcEvent<S>),
    Pressed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Pressed;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Delivery {
    Droppable,
    Guaranteed,
}

impl<S> Event<S> {
    pub const fn delivery(&self) -> Delivery {
        match self {
            Self::Blink(BlinkEvent::Tick) => Delivery::Droppable,
            Self::Blink(BlinkEvent::Toggle)
            | Self::Button(ButtonEvent::Edge | ButtonEvent::Settled)
            | Self::Adc(
                AdcEvent::Start | AdcEvent::Stop | AdcEvent::Samples(_) | AdcEvent::Overrun,
            )
            | Self::Pressed => Delivery::Guaranteed,
        }
    }
}

impl<B: Board> Machine for App<B> {
    type Event = Event<Samples<B>>;

    fn run(self, event: Self::Event) -> Self {
        match event {
            Event::Blink(event) => Self {
                blink: self.blink.run(event),
                ..self
            },
            Event::Button(event) => Self {
                button: self.button.run(event),
                ..self
            },
            Event::Adc(event) => Self {
                adc: self.adc.run(event),
                ..self
            },
            Event::Pressed => {
                Self::following(self.blink.run(BlinkEvent::Toggle), self.button, self.adc)
            }
        }
    }
}

impl<B: Board> App<B> {
    fn following(
        blink: Blink<B::Led, B::BlinkTimer>,
        button: Button<B::Button, B::DebounceTimer, B::Presses>,
        adc: Adc<B::Sampler, B::Reports>,
    ) -> Self {
        Self {
            adc: adc.run(match blink.activity() {
                Activity::Running => AdcEvent::Start,
                Activity::Paused => AdcEvent::Stop,
            }),
            blink,
            button,
        }
    }

    pub fn start(
        led: B::Led,
        blink_timer: B::BlinkTimer,
        button: B::Button,
        debounce_timer: B::DebounceTimer,
        presses: B::Presses,
        sampler: B::Sampler,
        reports: B::Reports,
    ) -> Self {
        Self {
            blink: Blink::start(led, blink_timer),
            button: Button::start(button, debounce_timer, presses),
            adc: Adc::start(sampler, reports),
        }
    }
}

pub trait Timer: Sized {
    fn started(self) -> Self;
    fn stopped(self) -> Self;
}

pub trait EdgeInput: Sized {
    fn armed(self) -> Self;
    fn disarmed(self) -> Self;
    fn sampled(self) -> (PinState, Self);
}

pub trait Post<E>: Sized {
    fn posted(self, event: E) -> Self;
}

pub trait Sampler: Sized {
    type Samples: Deref<Target = [u16]>;

    fn started(self) -> Self;
    fn stopped(self) -> Self;
}

#[cfg(feature = "defmt")]
impl<B: Board> defmt::Format for App<B> {
    fn format(&self, f: defmt::Formatter) {
        defmt::write!(f, "{} {} {}", self.blink, self.button, self.adc)
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::mock::{
        MockBoard, MockLed, MockPin, MockPresses, MockReports, MockSampler, MockTimer,
    };
    use super::{
        Adc, AdcEvent, App, Blink, BlinkEvent, Button, ButtonEvent, Delivery, Event, Machine,
    };
    use embedded_hal::digital::PinState;
    use std::vec::Vec;

    fn started() -> App<MockBoard> {
        App::start(
            MockLed(PinState::Low),
            MockTimer::Stopped,
            MockPin::disarmed(PinState::Low),
            MockTimer::Stopped,
            MockPresses::default(),
            MockSampler::Stopped,
            MockReports::default(),
        )
    }

    #[test]
    fn pressed_toggles_the_blink_and_the_adc_follows_its_activity() {
        let App { blink, button, adc } = started().run(Event::Pressed);
        let expected = started();
        assert_eq!(blink, expected.blink.run(BlinkEvent::Toggle));
        assert_eq!(button, expected.button);
        assert_eq!(adc, expected.adc.run(AdcEvent::Stop));
    }

    #[test]
    fn after_an_overrun_two_presses_bring_blink_and_adc_back_in_step() {
        let App { blink, adc, .. } = [
            Event::Adc(AdcEvent::Overrun),
            Event::Pressed,
            Event::Pressed,
        ]
        .into_iter()
        .fold(started(), App::run);
        let expected = started();
        assert_eq!(
            blink,
            expected
                .blink
                .run(BlinkEvent::Toggle)
                .run(BlinkEvent::Toggle)
        );
        assert_eq!(
            adc,
            expected
                .adc
                .run(AdcEvent::Overrun)
                .run(AdcEvent::Stop)
                .run(AdcEvent::Start)
        );
    }

    #[test]
    fn each_region_event_reaches_only_its_region() {
        let App { blink, button, adc } = started().run(Event::Button(ButtonEvent::Edge));
        let expected = started();
        assert_eq!(blink, expected.blink);
        assert_eq!(button, expected.button.run(ButtonEvent::Edge));
        assert_eq!(adc, expected.adc);

        let App { blink, button, adc } =
            started().run(Event::Adc(AdcEvent::Samples(Vec::from([7]))));
        let expected = started();
        assert_eq!(blink, expected.blink);
        assert_eq!(button, expected.button);
        assert_eq!(adc, expected.adc.run(AdcEvent::Samples(Vec::from([7]))));
    }

    #[test]
    fn start_blinks_samples_and_arms_the_button_at_its_sampled_level() {
        let App { blink, button, adc } = started();
        assert_eq!(
            blink,
            Blink::start(MockLed(PinState::Low), MockTimer::Stopped)
        );
        assert_eq!(
            button,
            Button::start(
                MockPin::disarmed(PinState::Low),
                MockTimer::Stopped,
                MockPresses::default()
            )
        );
        assert_eq!(
            adc,
            Adc::start(MockSampler::Stopped, MockReports::default())
        );
    }

    #[test]
    fn only_blink_ticks_may_be_dropped_because_the_next_tick_supersedes_them() {
        [
            (Event::Blink(BlinkEvent::Tick), Delivery::Droppable),
            (Event::Blink(BlinkEvent::Toggle), Delivery::Guaranteed),
            (Event::Button(ButtonEvent::Edge), Delivery::Guaranteed),
            (Event::Button(ButtonEvent::Settled), Delivery::Guaranteed),
            (Event::Adc(AdcEvent::Start), Delivery::Guaranteed),
            (Event::Adc(AdcEvent::Stop), Delivery::Guaranteed),
            (
                Event::Adc(AdcEvent::Samples(Vec::from([1]))),
                Delivery::Guaranteed,
            ),
            (Event::Adc(AdcEvent::Overrun), Delivery::Guaranteed),
            (Event::Pressed, Delivery::Guaranteed),
        ]
        .into_iter()
        .for_each(|(event, expected)| assert_eq!(event.delivery(), expected, "{event:?}"));
    }
}
