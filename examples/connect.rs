#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]
#![allow(clippy::cast_lossless)]
#![allow(clippy::as_conversions)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::arithmetic_side_effects)]

use std::{
    f32::consts::TAU,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

extern crate zsars;

fn main() {
    // On ctrl+c, allow the keyboard to be reset to how the user defined it.
    static CANCEL: AtomicBool = AtomicBool::new(false);
    ctrlc::set_handler(move || CANCEL.store(true, Ordering::Relaxed))
        .expect("couldnt set ctrl-c handler");

    // connect to the first available keyboard
    let keyboard = zsars::Keyboard::connect_to_any().unwrap().unwrap();

    let start = Instant::now();
    while !CANCEL.load(Ordering::Relaxed) {
        // make the first 6 keys animate in a rainbow
        for i in 0..6 {
            rainbow(start, &keyboard, i, TAU * (i as f32) / 6.);
        }

        // and also toggle the status leds on and off
        let max_led = keyboard.max_status_led_index();
        let target_led = (start.elapsed().as_secs() % ((max_led as u64 + 1) * 2)) as u8;
        let disable_it = target_led > max_led;
        keyboard
            .set_status_led(target_led % (max_led + 1), !disable_it)
            .unwrap();

        std::thread::sleep(Duration::from_millis(100));
    }

    // Go back to how the user set it
    keyboard.take_rgb_control(false).unwrap();
    keyboard.take_led_control(false).unwrap();
}

fn rainbow(start: Instant, keyboard: &zsars::Keyboard, index: u8, offset: f32) {
    let offset = start.elapsed().as_secs_f32() + offset;
    let r = (offset.cos() * 255.).max(0.) as u8;
    let g = ((offset - TAU / 3.).cos() * 255.).max(0.) as u8;
    let b = ((offset - TAU * 2. / 3.).cos() * 255.).max(0.) as u8;
    keyboard.set_rgb_led(index, r, g, b).unwrap();
}
