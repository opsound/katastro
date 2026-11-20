//! A slow or hidden review window needs only the newest complete snapshot.
use super::Snapshot;
use crate::updates::Wake;
use std::{
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, RecvTimeoutError, TryRecvError},
    },
    time::Duration,
};

pub struct Snapshots {
    pending: Arc<Mutex<Option<Snapshot>>>,
    ready: Receiver<()>,
}

impl Snapshots {
    pub(super) fn channel(wake: Wake) -> (impl Fn(Snapshot) + Send, Self) {
        let pending = Arc::new(Mutex::new(None));
        let (notify, ready) = mpsc::sync_channel(1);
        let receiver = Self {
            pending: Arc::clone(&pending),
            ready,
        };
        let publish = move |snapshot| {
            let previous = pending.lock().unwrap().replace(snapshot);
            // Only an empty-to-full transition needs a token and a repaint.
            if previous.is_none()
                && notify.try_send(()).is_ok()
                && let Some(wake) = &wake
            {
                wake();
            }
        };
        (publish, receiver)
    }

    pub fn try_recv(&self) -> Result<Snapshot, TryRecvError> {
        self.ready.try_recv()?;
        Ok(self.take_ready())
    }

    pub fn recv_timeout(&self, timeout: Duration) -> Result<Snapshot, RecvTimeoutError> {
        self.ready.recv_timeout(timeout)?;
        Ok(self.take_ready())
    }

    fn take_ready(&self) -> Snapshot {
        // Consume the token before clearing the slot. A concurrent publication
        // either replaces this value or finds an empty slot and signals again.
        self.pending
            .lock()
            .unwrap()
            .take()
            .expect("notified snapshot")
    }
}
