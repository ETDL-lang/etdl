//! A misbehaving fixture, hand-rolled without the `etdl_supplement!`
//! macro: `etdl_supplement_validate` returns bytes that aren't valid JSON
//! at all. Proves the host treats a malformed response as a clean
//! diagnostic, not a panic.

#[no_mangle]
pub extern "C" fn etdl_alloc(len: u32) -> u32 {
    etdl_supplement_sdk::__alloc(len)
}

#[no_mangle]
pub extern "C" fn etdl_dealloc(ptr: u32, len: u32) {
    etdl_supplement_sdk::__dealloc(ptr, len)
}

#[no_mangle]
pub extern "C" fn etdl_supplement_id() -> u64 {
    etdl_supplement_sdk::__ret_str("etdl.fixture-malformed")
}

#[no_mangle]
pub extern "C" fn etdl_supplement_version() -> u64 {
    etdl_supplement_sdk::__ret_str("1.0")
}

#[no_mangle]
pub extern "C" fn etdl_supplement_validate(_a: u32, _b: u32, _c: u32, _d: u32) -> u64 {
    etdl_supplement_sdk::__ret_bytes(b"not json at all {{{".to_vec())
}

#[no_mangle]
pub extern "C" fn etdl_supplement_process(_a: u32, _b: u32, _c: u32, _d: u32) -> u64 {
    etdl_supplement_sdk::__ret_bytes(b"also not json".to_vec())
}

fn main() {}
