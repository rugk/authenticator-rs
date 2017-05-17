use std::io;
use std::sync::mpsc::{channel, Sender, Receiver, TryIter};
use std::thread;

use super::iohid::*;
use super::iokit::*;
use core_foundation_sys::runloop::*;
use runloop::RunLoop;

extern crate log;
extern crate libc;
use libc::c_void;

pub enum Event {
    Add { device_id: IOHIDDeviceID },
    Remove { device_id: IOHIDDeviceID },
}

pub struct Monitor {
    // Receive events from the thread.
    rx: Receiver<Event>,
    // Handle to the thread loop.
    thread: RunLoop
}

impl Monitor {
    pub fn new() -> io::Result<Self> {
        let (tx, rx) = channel();

        let thread = RunLoop::new(move |alive| {
            let tx_box = Box::new(tx);
            let tx_ptr = Box::into_raw(tx_box) as *mut libc::c_void;

            // This will keep `tx` alive only for the scope.
            let _tx = unsafe { Box::from_raw(tx_ptr) };

            // Create and initialize a scoped HID manager.
            let manager = IOHIDManager::new()?;

            // Match only U2F devices.
            let dict = IOHIDDeviceMatcher::new();
            unsafe { IOHIDManagerSetDeviceMatching(manager.get(), dict.get()) };

            // Register callbacks.
            unsafe {
                IOHIDManagerRegisterDeviceMatchingCallback(
                    manager.get(), Monitor::device_add_cb, tx_ptr);
                IOHIDManagerRegisterDeviceRemovalCallback(
                    manager.get(), Monitor::device_remove_cb, tx_ptr);
            }

            // Run the Event Loop. CFRunLoopRunInMode() will dispatch HID
            // input reports into the various callbacks
            while alive() {
                trace!("OSX Runloop running, handle={:?}", thread::current());

                if unsafe { CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.1, 0) } == kCFRunLoopRunStopped {
                    info!("OSX Runloop device stopped.");
                    break;
                }
            }

            Ok(())
        }, 0 /* no timeout */)?;

        Ok(Self { rx, thread })
    }

    pub fn events<'a>(&'a self) -> TryIter<'a, Event> {
        self.rx.try_iter()
    }

    // This might block.
    pub fn stop(&mut self) {
        self.thread.cancel();
    }

    extern "C" fn device_add_cb(context: *mut c_void, _: IOReturn,
                                _: *mut c_void, device: IOHIDDeviceRef) {
        let tx = unsafe { &*(context as *mut Sender<Event>) };
        let _ = tx.send(Event::Add {
            device_id: IOHIDDeviceID::from_ref(device)
        });
    }

    extern "C" fn device_remove_cb(context: *mut c_void, _: IOReturn,
                                   _: *mut c_void, device: IOHIDDeviceRef) {
        let tx = unsafe { &*(context as *mut Sender<Event>) };
        let _ = tx.send(Event::Remove {
            device_id: IOHIDDeviceID::from_ref(device)
        });
    }
}
