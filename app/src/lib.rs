#![no_std]

mod blink;
mod button;
#[cfg(test)]
mod mock;

pub use blink::{Blink, BlinkEvent};
pub use button::{Button, ButtonEvent};

use core::convert::Infallible;
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
    type Events: Post<Event>;
}

pub struct App<B: Board> {
    blink: Blink<B::Led, B::BlinkTimer>,
    button: Button<B::Button, B::DebounceTimer, B::Events>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Event {
    Blink(BlinkEvent),
    Button(ButtonEvent),
    Pressed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Delivery {
    Droppable,
    Guaranteed,
}

impl Event {
    pub const fn delivery(self) -> Delivery {
        match self {
            Self::Blink(BlinkEvent::Tick) => Delivery::Droppable,
            Self::Blink(BlinkEvent::Toggle)
            | Self::Button(ButtonEvent::Edge | ButtonEvent::Settled)
            | Self::Pressed => Delivery::Guaranteed,
        }
    }
}

impl<B: Board> Machine for App<B> {
    type Event = Event;

    fn run(self, event: Event) -> Self {
        match event {
            Event::Blink(event) => Self {
                blink: self.blink.run(event),
                ..self
            },
            Event::Button(event) => Self {
                button: self.button.run(event),
                ..self
            },
            Event::Pressed => Self {
                blink: self.blink.run(BlinkEvent::Toggle),
                ..self
            },
        }
    }
}

impl<B: Board> App<B> {
    pub fn start(
        led: B::Led,
        blink_timer: B::BlinkTimer,
        button: B::Button,
        debounce_timer: B::DebounceTimer,
        events: B::Events,
    ) -> Self {
        Self {
            blink: Blink::start(led, blink_timer),
            button: Button::start(button, debounce_timer, events),
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

#[cfg(feature = "defmt")]
impl<B: Board> defmt::Format for App<B> {
    fn format(&self, f: defmt::Formatter) {
        defmt::write!(f, "{} {}", self.blink, self.button)
    }
}

#[cfg(test)]
mod tests {
    use super::mock::{MockBoard, MockEvents, MockLed, MockPin, MockTimer};
    use super::{App, Blink, BlinkEvent, Button, ButtonEvent, Delivery, Event, Machine};
    use embedded_hal::digital::PinState;

    fn started() -> App<MockBoard> {
        App::start(
            MockLed(PinState::Low),
            MockTimer::Stopped,
            MockPin::disarmed(PinState::Low),
            MockTimer::Stopped,
            MockEvents::default(),
        )
    }

    #[test]
    fn pressed_is_routed_to_the_blink_region_as_a_toggle() {
        let App { blink, button } = started().run(Event::Pressed);
        let App {
            blink: expected_blink,
            button: expected_button,
        } = started();
        assert_eq!(blink, expected_blink.run(BlinkEvent::Toggle));
        assert_eq!(button, expected_button);
    }

    #[test]
    fn button_events_reach_only_the_button_region() {
        let App { blink, button } = started().run(Event::Button(ButtonEvent::Edge));
        let App {
            blink: expected_blink,
            button: expected_button,
        } = started();
        assert_eq!(blink, expected_blink);
        assert_eq!(button, expected_button.run(ButtonEvent::Edge));
    }

    #[test]
    fn start_blinks_and_arms_the_button_at_its_sampled_level() {
        let App { blink, button } = started();
        assert_eq!(
            blink,
            Blink::start(MockLed(PinState::Low), MockTimer::Stopped)
        );
        assert_eq!(
            button,
            Button::start(
                MockPin::disarmed(PinState::Low),
                MockTimer::Stopped,
                MockEvents::default()
            )
        );
    }

    #[test]
    fn only_blink_ticks_may_be_dropped_because_the_next_tick_supersedes_them() {
        [
            (Event::Blink(BlinkEvent::Tick), Delivery::Droppable),
            (Event::Blink(BlinkEvent::Toggle), Delivery::Guaranteed),
            (Event::Button(ButtonEvent::Edge), Delivery::Guaranteed),
            (Event::Button(ButtonEvent::Settled), Delivery::Guaranteed),
            (Event::Pressed, Delivery::Guaranteed),
        ]
        .into_iter()
        .for_each(|(event, expected)| assert_eq!(event.delivery(), expected, "{event:?}"));
    }
}
