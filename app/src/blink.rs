use crate::{Machine, Timer};
use core::convert::Infallible;
use embedded_hal::digital::{OutputPin, PinState};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum BlinkEvent {
    Toggle,
    Tick,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Blink<L, T> {
    Blinking(Blinking<L, T>),
    Paused(Paused<L, T>),
}

impl<L, T> Machine for Blink<L, T>
where
    L: OutputPin<Error = Infallible>,
    T: Timer,
{
    type Event = BlinkEvent;

    fn run(self, event: BlinkEvent) -> Self {
        match (self, event) {
            (Self::Blinking(blinking), BlinkEvent::Toggle) => Self::Paused(blinking.pause()),
            (Self::Paused(paused), BlinkEvent::Toggle) => Self::Blinking(paused.resume()),
            (Self::Blinking(blinking), BlinkEvent::Tick) => Self::Blinking(blinking.toggle()),
            (Self::Paused(paused), BlinkEvent::Tick) => Self::Paused(paused),
        }
    }
}

impl<L, T> Blink<L, T>
where
    L: OutputPin<Error = Infallible>,
    T: Timer,
{
    pub fn start(led: L, timer: T) -> Self {
        Self::Blinking(Blinking::start(led, timer))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Activity {
    Running,
    Paused,
}

impl<L, T> Blink<L, T> {
    pub(crate) const fn activity(&self) -> Activity {
        match self {
            Self::Blinking(_) => Activity::Running,
            Self::Paused(_) => Activity::Paused,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Blinking<L, T> {
    led: L,
    timer: T,
    phase: Led,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Paused<L, T> {
    led: L,
    timer: T,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Led {
    On,
    Off,
}

impl<L, T> Blinking<L, T>
where
    L: OutputPin<Error = Infallible>,
    T: Timer,
{
    fn start(led: L, timer: T) -> Self {
        Self::showing(led, timer.started(), Led::On)
    }

    fn pause(self) -> Paused<L, T> {
        Paused {
            timer: self.timer.stopped(),
            led: lit(self.led, Led::Off),
        }
    }

    fn toggle(self) -> Self {
        Self::showing(self.led, self.timer, self.phase.toggled())
    }

    fn showing(led: L, timer: T, phase: Led) -> Self {
        Self {
            led: lit(led, phase),
            timer,
            phase,
        }
    }
}

impl<L, T> Paused<L, T>
where
    L: OutputPin<Error = Infallible>,
    T: Timer,
{
    fn resume(self) -> Blinking<L, T> {
        Blinking::start(self.led, self.timer)
    }
}

fn lit<L: OutputPin<Error = Infallible>>(mut led: L, phase: Led) -> L {
    let Ok(()) = led.set_state(phase.into());
    led
}

impl Led {
    const fn toggled(self) -> Self {
        match self {
            Self::On => Self::Off,
            Self::Off => Self::On,
        }
    }
}

impl From<Led> for PinState {
    fn from(led: Led) -> Self {
        match led {
            Led::On => Self::High,
            Led::Off => Self::Low,
        }
    }
}

#[cfg(feature = "defmt")]
impl<L, T> defmt::Format for Blink<L, T> {
    fn format(&self, f: defmt::Formatter) {
        match self {
            Self::Blinking(blinking) => defmt::write!(f, "Blinking({})", blinking.phase),
            Self::Paused(_) => defmt::write!(f, "Paused"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Blink, BlinkEvent, Blinking, Led, Paused};
    use crate::Machine;
    use crate::mock::{MockLed, MockTimer};
    use embedded_hal::digital::PinState;

    const fn blinking(led: PinState, phase: Led) -> Blink<MockLed, MockTimer> {
        Blink::Blinking(Blinking {
            led: MockLed(led),
            timer: MockTimer::Started,
            phase,
        })
    }

    const fn paused() -> Blink<MockLed, MockTimer> {
        Blink::Paused(Paused {
            led: MockLed(PinState::Low),
            timer: MockTimer::Stopped,
        })
    }

    #[test]
    fn start_lights_the_led_and_starts_the_timer() {
        assert_eq!(
            Blink::start(MockLed(PinState::Low), MockTimer::Stopped),
            blinking(PinState::High, Led::On)
        );
    }

    #[test]
    fn every_state_event_pair_drives_the_hardware_as_specified() {
        [
            (
                blinking(PinState::High, Led::On),
                BlinkEvent::Toggle,
                paused(),
            ),
            (
                blinking(PinState::Low, Led::Off),
                BlinkEvent::Toggle,
                paused(),
            ),
            (
                paused(),
                BlinkEvent::Toggle,
                blinking(PinState::High, Led::On),
            ),
            (
                blinking(PinState::High, Led::On),
                BlinkEvent::Tick,
                blinking(PinState::Low, Led::Off),
            ),
            (
                blinking(PinState::Low, Led::Off),
                BlinkEvent::Tick,
                blinking(PinState::High, Led::On),
            ),
            (paused(), BlinkEvent::Tick, paused()),
        ]
        .into_iter()
        .for_each(|(state, event, expected)| assert_eq!(state.run(event), expected));
    }
}
