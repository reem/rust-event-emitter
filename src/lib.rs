#![feature(unboxed_closures)]
#![license = "MIT"]
//#![deny(missing_doc)]
#![deny(warnings)]

//! An asynchronous EventEmitter based on rust-event
//!
//! You must initialize the rust-event EventQueue in your
//! own code.
//!

extern crate uuid;
extern crate event;
extern crate typemap;
extern crate "unsafe-any" as uany;
extern crate forever;

// Used to track EventEmitters
use uuid::Uuid;

// Used to store events and handlers.
use typemap::{TypeMap, Assoc};

// Used to transmute down from data in a TypeMap
use uany::UncheckedAnyDowncast;

// Holds the emitter lookup table
use forever::Forever;

use std::intrinsics::TypeId;
use std::sync::{RWLock, Arc, Once, ONCE_INIT};
use std::collections::HashMap;
use std::mem;

pub use event::Handler;

static mut SETUP: Once = ONCE_INIT;

#[deriving(Clone)]
pub struct EventEmitter {
    id: Uuid,
    handlers: Arc<RWLock<TypeMap>>,
    alive: Option<Arc<EmitterGuard>>
}

impl EventEmitter {
    pub fn new() -> EventEmitter {
        unsafe { SETUP.doit(setup) };

        let id = Uuid::new_v4();

        let emitter = EventEmitter {
            id: id,
            handlers: Arc::new(RWLock::new(TypeMap::new())),
            alive: Some(Arc::new(EmitterGuard { id: id }))
        };

        let mut posted = emitter.clone();
        // Don't hold on to the EmitterGuard in the global emitters dispatcher.
        posted.alive = None;

        // Create a new EventEmitterCreated event.
        event::Event::new(posted).trigger::<EventEmitterCreated>();

        // Try to consume the just-added event.
        event::queue().trigger();

        emitter
    }

    pub fn on<K: Assoc<X>, X>(&self, handler: Handler<X>) {
        self.handlers.write().insert::<EmitterEventKey<K, X>, Handler<X>>(handler);
    }

    pub fn trigger<K: Assoc<X>, X>(&self, data: X) {
        event::Event::new(EventEmitterEvent {
            emitter_id: self.id,
            event_id: TypeId::of::<EmitterEventKey<K, X>>(),
            data: unsafe { mem::transmute(box data) }
        }).trigger::<EventEmitterEvent>()
    }
}

impl Drop for EmitterGuard {
    fn drop(&mut self) {
        event::Event::new(self.id).trigger::<EventEmitterDropped>();
    }
}

struct EventEmitterEvent {
    // The id of the emitter to find it in `emitters()`
    emitter_id: Uuid,

    // TypeId::of::<EmitterEventKey<WhateverEvent, EventData>>()
    //
    // Used to lookup the correct handler in the emitter.
    event_id: TypeId,

    // Opaque pointer to some data.
    //
    // I'm sorry.
    data: *mut ()
}

impl Assoc<EventEmitterEvent> for EventEmitterEvent {}

fn setup() {
    let emitters = Forever::new(RWLock::new(HashMap::new()));
    let emitters_insert = emitters.clone();
    let emitters_delete = emitters.clone();

    // Dispatch EventEmitter events.
    event::on::<EventEmitterEvent, EventEmitterEvent>(box() (|&: box event: Box<EventEmitterEvent>| {
        emitters.read().find(&event.emitter_id).map(|emitter: &Box<EventEmitter>| {
            let read = emitter.handlers.read();
            match unsafe { read.data() }.find(&event.event_id) {
                Some(handler) => {
                    unsafe {
                        // The handler still thinks it receives a Box, so it will drop
                        // the event data for us.
                        handler.downcast_ref_unchecked::<&Fn<(*mut (),), ()>>()
                    }.call((event.data,));
                },
                _ => {}
            }
        });
    }) as event::Handler<EventEmitterEvent>);

    // Add new EventEmitters.
    event::on::<EventEmitterCreated, EventEmitter>(box() (|&: emitter: Box<EventEmitter>| {
        emitters_insert.write().insert(emitter.id, emitter);
    }) as event::Handler<EventEmitter>);

    // Delete dropped EventEmitters.
    event::on::<EventEmitterDropped, Uuid>(box() (|&: box id: Box<Uuid>| {
        emitters_delete.write().remove(&id);
    }) as Handler<Uuid>);
}

struct EventEmitterCreated;

impl Assoc<EventEmitter> for EventEmitterCreated {}

struct EmitterEventKey<K, X>;

impl<K, X> Assoc<Handler<X>> for EmitterEventKey<K, X> {}

struct EventEmitterDropped;

impl Assoc<Uuid> for EventEmitterDropped {}

struct EmitterGuard { id: Uuid }

