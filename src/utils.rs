use axum::http::HeaderMap;
use std::{net::IpAddr, sync::Mutex};
use tokio::sync::Notify;

#[inline]
pub fn extract_ip(headers: &HeaderMap) -> Option<IpAddr> {
    let ip = headers
        .get("x-real-ip")
        .or_else(|| headers.get("x-forwarded-for"))
        .map(|ip| ip.to_str().unwrap_or_default())
        .unwrap_or_default();

    if ip.is_empty() {
        return None;
    }

    let ip = if ip.contains(',') {
        ip.split(',').next().unwrap_or_default().trim().to_string()
    } else {
        ip.to_string()
    };

    ip.parse().ok()
}

pub struct EvictingMutex {
    state: Mutex<State>,
    notify: Notify,
}

struct State {
    locked: bool,
    latest_ticket: u64,
}

pub struct EvictingMutexGuard<'a> {
    mutex: &'a EvictingMutex,
}

#[derive(Debug)]
pub struct Aborted;

impl EvictingMutex {
    pub const fn new() -> Self {
        Self {
            state: Mutex::new(State {
                locked: false,
                latest_ticket: 0,
            }),
            notify: Notify::const_new(),
        }
    }

    pub async fn acquire(&self) -> Result<EvictingMutexGuard<'_>, Aborted> {
        let my_ticket = {
            let mut state = self.state.lock().unwrap();
            state.latest_ticket += 1;

            self.notify.notify_waiters();
            state.latest_ticket
        };

        loop {
            let notified = self.notify.notified();

            {
                let mut state = self.state.lock().unwrap();

                if state.latest_ticket != my_ticket {
                    return Err(Aborted);
                }

                if !state.locked {
                    state.locked = true;
                    return Ok(EvictingMutexGuard { mutex: self });
                }
            }

            notified.await;
        }
    }
}

impl Drop for EvictingMutexGuard<'_> {
    fn drop(&mut self) {
        let mut state = self.mutex.state.lock().unwrap();
        state.locked = false;
        self.mutex.notify.notify_waiters();
    }
}
