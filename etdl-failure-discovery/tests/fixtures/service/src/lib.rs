//! Mixed service example: a payment capture path exercising many failure
//! pattern classes. This is fixture source — it intentionally contains
//! `unwrap()`, `expect()`, `panic!`, etc.

use std::sync::mpsc;
use std::sync::Mutex;

/// Custom error type (pattern 18).
#[derive(Debug)]
pub enum PaymentError {
    Timeout,
    Rejected,
    InvalidResponse,
    Database,
}

/// Result propagation (pattern 1) and `?` (pattern 2).
pub fn capture(
    amount: u64,
    client: &reqwest::blocking::Client,
    tx: &mpsc::Sender<String>,
    db: &Mutex<()>,
) -> Result<(), PaymentError> {
    let body = std::fs::read_to_string("config.json").unwrap(); // pattern 11 + 5
    let _ = serde_json::from_str::<serde_json::Value>(&body).unwrap(); // pattern 13 + 5
    let resp = client
        .get("https://api.example.com/charge")
        .send() // pattern 12
        .map_err(|_| PaymentError::Timeout)? // pattern 2
        .error_for_status()
        .map_err(|_| PaymentError::Rejected)?;
    let parsed: serde_json::Value =
        serde_json::from_str(&resp.text().unwrap()).expect("bad json"); // pattern 13 + 6
    let amount_s = parsed["amount"].as_str().unwrap_or(""); // pattern 7 + 5
    let n: u64 = amount_s.parse().map_err(|_| PaymentError::InvalidResponse)?; // pattern 10
    if n == 0 {
        return Err(PaymentError::Rejected); // pattern 3
    }
    if n > 1000 {
        panic!("amount too large"); // pattern 4
    }
    assert!(n < 10_000); // pattern 8
    let _guard = db.lock().unwrap(); // pattern 15 + 5
    tx.send(format!("charged {n}")).unwrap(); // pattern 14 + 5
    let idx: usize = n as usize;
    let slice = &[1u8, 2, 3];
    let _v = slice[idx]; // pattern 7 (indexing)
    let _q = 100 / (idx + 0); // pattern 9 (division, may be zero at runtime)
    let _ = std::thread::spawn(move || {});
    Ok(())
}

pub fn unreachable_path() -> i32 {
    unreachable!() // pattern 4 (unreachable)
}

pub fn todo_path() -> i32 {
    todo!() // pattern 4 (unimplemented)
}
