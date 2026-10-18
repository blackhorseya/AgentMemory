//! System clock and random identifiers.

use base64::Engine;
use chrono::{DateTime, Utc};
use memory_application::ports::system::{Clock, IdGenerator};
use rand::rngs::OsRng;
use rand::{Rng, RngCore};

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

pub struct RandomIds;

impl IdGenerator for RandomIds {
    fn next(&self, prefix: &str) -> String {
        format!("{prefix}{}", uuid::Uuid::new_v4().simple())
    }

    fn hex(&self, n: usize) -> String {
        let mut rng = rand::thread_rng();
        (0..n)
            .map(|_| char::from_digit(rng.gen_range(0..16), 16).unwrap_or('0'))
            .collect()
    }

    fn secret(&self, bytes: usize) -> String {
        let mut buf = vec![0u8; bytes];
        OsRng.fill_bytes(&mut buf);
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buf)
    }
}
