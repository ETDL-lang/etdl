//! A third-party dependency file. Discovery must not treat its failures as
//! first-party application failures by default.

pub fn third_party_thing() -> Result<(), String> {
    let x = unsafe_dependency().unwrap();
    let _ = x + 1;
    Ok(())
}

fn unsafe_dependency() -> Option<i32> {
    None
}
