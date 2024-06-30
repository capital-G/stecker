#[no_mangle]
pub extern "C" fn recv_message() -> f32 {
    32.
}

#[cxx::bridge]
mod ffi {

    extern "Rust" {
        fn recv_message() -> f32;
    }
}
