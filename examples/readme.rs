#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]
#![allow(clippy::cast_lossless)]
#![allow(clippy::as_conversions)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::arithmetic_side_effects)]
extern crate zsars;

fn main() {
    // connect to the first available keyboard
    let mut keyboard = zsars::Keyboard::connect_to_any().unwrap().unwrap();

    keyboard.set_layer(3).unwrap();
    keyboard.set_rgb_led(20, 255, 255, 0).unwrap();
    let await_anything = keyboard.read_blocking();
    dbg!(&await_anything);
}
