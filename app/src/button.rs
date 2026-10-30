use crate::{EdgeInput, Event, Machine, Post, Timer};
use embedded_hal::digital::PinState;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum ButtonEvent {
    Edge,
    Settled,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Button<P, D, Q> {
    Stable(Stable<P, D, Q>),
    Settling(Settling<P, D, Q>),
}

impl<P, D, Q> Machine for Button<P, D, Q>
where
    P: EdgeInput,
    D: Timer,
    Q: Post<Event>,
{
    type Event = ButtonEvent;

    fn run(self, event: ButtonEvent) -> Self {
        match (self, event) {
            (Self::Stable(stable), ButtonEvent::Edge) => Self::Settling(stable.settle()),
            (Self::Settling(settling), ButtonEvent::Settled) => settling.resample(),
            (Self::Settling(settling), ButtonEvent::Edge) => Self::Settling(settling),
            (Self::Stable(stable), ButtonEvent::Settled) => Self::Stable(stable),
        }
    }
}

impl<P, D, Q> Button<P, D, Q>
where
    P: EdgeInput,
    D: Timer,
    Q: Post<Event>,
{
    pub fn start(pin: P, timer: D, events: Q) -> Self {
        Self::Stable(Stable::at(pin.armed().sampled(), timer, events))
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Stable<P, D, Q> {
    pin: P,
    timer: D,
    events: Q,
    level: PinState,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Settling<P, D, Q> {
    pin: P,
    timer: D,
    events: Q,
    from: PinState,
    previous: PinState,
}

impl<P, D, Q> Stable<P, D, Q>
where
    P: EdgeInput,
    D: Timer,
    Q: Post<Event>,
{
    fn at((level, pin): (PinState, P), timer: D, events: Q) -> Self {
        Self {
            pin,
            timer,
            events,
            level,
        }
    }

    fn settle(self) -> Settling<P, D, Q> {
        Settling {
            pin: self.pin.disarmed(),
            timer: self.timer.started(),
            events: self.events,
            from: self.level,
            previous: self.level,
        }
    }
}

impl<P, D, Q> Settling<P, D, Q>
where
    P: EdgeInput,
    D: Timer,
    Q: Post<Event>,
{
    fn resample(self) -> Button<P, D, Q> {
        Self::judged(
            self.pin.armed().sampled(),
            self.timer,
            self.events,
            self.from,
            self.previous,
        )
    }

    fn judged(
        (level, pin): (PinState, P),
        timer: D,
        events: Q,
        from: PinState,
        previous: PinState,
    ) -> Button<P, D, Q> {
        match level == previous {
            true => Button::Stable(Stable::at(
                (level, pin),
                timer.stopped(),
                posted_if_pressed(events, from, level),
            )),
            false => Button::Settling(Self {
                pin: pin.disarmed(),
                timer: timer.started(),
                events,
                from,
                previous: level,
            }),
        }
    }
}

fn posted_if_pressed<Q: Post<Event>>(events: Q, from: PinState, to: PinState) -> Q {
    match (from, to) {
        (PinState::Low, PinState::High) => events.posted(Event::Pressed),
        (PinState::Low, PinState::Low)
        | (PinState::High, PinState::High)
        | (PinState::High, PinState::Low) => events,
    }
}

#[cfg(feature = "defmt")]
impl<P, D, Q> defmt::Format for Button<P, D, Q> {
    fn format(&self, f: defmt::Formatter) {
        match self {
            Self::Stable(stable) => defmt::write!(f, "Stable({})", stable.level),
            Self::Settling(settling) => defmt::write!(
                f,
                "Settling(from {}, last {})",
                settling.from,
                settling.previous
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Button, ButtonEvent, Settling, Stable};
    use crate::mock::{MockEvents, MockPin, MockTimer};
    use crate::{Event, Machine};
    use embedded_hal::digital::PinState;

    type MockButton = Button<MockPin, MockTimer, MockEvents>;

    fn stable(level: PinState, timer: MockTimer, posted: &[Event]) -> MockButton {
        Button::Stable(Stable {
            pin: MockPin::armed(level),
            timer,
            events: MockEvents::from(posted),
            level,
        })
    }

    fn settling(from: PinState, previous: PinState, now: PinState) -> MockButton {
        Button::Settling(Settling {
            pin: MockPin::disarmed(now),
            timer: MockTimer::Started,
            events: MockEvents::default(),
            from,
            previous,
        })
    }

    fn with_pin_at(button: MockButton, now: PinState) -> MockButton {
        match button {
            Button::Stable(stable) => Button::Stable(Stable {
                pin: stable.pin.at(now),
                ..stable
            }),
            Button::Settling(settling) => Button::Settling(Settling {
                pin: settling.pin.at(now),
                ..settling
            }),
        }
    }

    #[test]
    fn start_arms_the_pin_and_adopts_its_level() {
        assert_eq!(
            Button::start(
                MockPin::disarmed(PinState::High),
                MockTimer::Stopped,
                MockEvents::default()
            ),
            stable(PinState::High, MockTimer::Stopped, &[])
        );
    }

    #[test]
    fn an_edge_disarms_the_pin_and_starts_the_debounce_timer() {
        assert_eq!(
            stable(PinState::Low, MockTimer::Stopped, &[]).run(ButtonEvent::Edge),
            settling(PinState::Low, PinState::Low, PinState::Low)
        );
    }

    #[test]
    fn a_level_commits_only_after_two_consecutive_equal_samples() {
        [
            (
                settling(PinState::Low, PinState::Low, PinState::Low),
                stable(PinState::Low, MockTimer::Stopped, &[]),
            ),
            (
                settling(PinState::Low, PinState::Low, PinState::High),
                settling(PinState::Low, PinState::High, PinState::High),
            ),
            (
                settling(PinState::Low, PinState::High, PinState::High),
                stable(PinState::High, MockTimer::Stopped, &[Event::Pressed]),
            ),
            (
                settling(PinState::Low, PinState::High, PinState::Low),
                settling(PinState::Low, PinState::Low, PinState::Low),
            ),
            (
                settling(PinState::High, PinState::Low, PinState::Low),
                stable(PinState::Low, MockTimer::Stopped, &[]),
            ),
            (
                settling(PinState::High, PinState::High, PinState::High),
                stable(PinState::High, MockTimer::Stopped, &[]),
            ),
        ]
        .into_iter()
        .for_each(|(state, expected)| assert_eq!(state.run(ButtonEvent::Settled), expected));
    }

    #[test]
    fn a_bounce_outlasting_one_debounce_period_posts_one_press() {
        assert_eq!(
            [
                (ButtonEvent::Edge, PinState::High),
                (ButtonEvent::Settled, PinState::High),
                (ButtonEvent::Settled, PinState::Low),
                (ButtonEvent::Settled, PinState::High),
                (ButtonEvent::Settled, PinState::High),
            ]
            .into_iter()
            .fold(
                stable(PinState::Low, MockTimer::Stopped, &[]),
                |button, (event, level)| with_pin_at(button, level).run(event)
            ),
            stable(PinState::High, MockTimer::Stopped, &[Event::Pressed])
        );
    }

    #[test]
    fn stale_events_leave_the_region_unchanged() {
        assert_eq!(
            settling(PinState::Low, PinState::Low, PinState::High).run(ButtonEvent::Edge),
            settling(PinState::Low, PinState::Low, PinState::High)
        );
        assert_eq!(
            stable(PinState::Low, MockTimer::Stopped, &[]).run(ButtonEvent::Settled),
            stable(PinState::Low, MockTimer::Stopped, &[])
        );
    }
}
