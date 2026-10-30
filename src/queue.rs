use crate::Event;
use app::{Post, Pressed};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;

const QUEUE_CAPACITY: usize = 8;
const GUARANTEED_IN_FLIGHT: usize = 4;

pub static EVENTS: Channel<CriticalSectionRawMutex, Event, QUEUE_CAPACITY> = Channel::new();

pub fn post(event: Event) {
    use app::Delivery;
    use embassy_sync::channel::TrySendError;

    critical_section::with(|_| {
        match (
            event.delivery(),
            EVENTS.len() < QUEUE_CAPACITY - GUARANTEED_IN_FLIGHT,
        ) {
            (Delivery::Droppable, false) => {}
            (Delivery::Droppable, true) | (Delivery::Guaranteed, _) => {
                match EVENTS.try_send(event) {
                    Ok(()) => {}
                    Err(TrySendError::Full(event)) => {
                        defmt::panic!("event queue full, lost guaranteed {}", event)
                    }
                }
            }
        }
    })
}

pub struct CentralQueue;

impl Post<Pressed> for CentralQueue {
    fn posted(self, _: Pressed) -> Self {
        post(Event::Pressed);
        self
    }
}
