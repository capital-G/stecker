#[no_mangle]
pub extern "C" fn recv_message(room_name: &str) -> f32 {
    println!("room name is {room_name}");
    32.
}

#[cxx::bridge]
mod ffi {

    extern "Rust" {
        fn recv_message(room_name: &str) -> f32;
    }
}
