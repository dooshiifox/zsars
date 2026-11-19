# ZSA Keyboard Rust Library

Control your ZSA keyboard programmatically without using
[Keymapp's API](https://github.com/zsa/kontroll/tree/main), instead going
straight over the cable.

## Usage

See the documentation at [https://docs.rs/zsars].

```
fn main() {
  // connect to the first available keyboard
  let mut keyboard = zsars::Keyboard::connect_to_any().unwrap().unwrap();

  keyboard.set_layer(3).unwrap();
  keyboard.set_rgb_led(20, 255, 255, 0).unwrap();
  let await_anything = keyboard.read_blocking();
  dbg!(&await_anything);
}
```

## How

This protocol isn't public, so it was an effort of reverse-engineering Keymapp
itself. Thankfully, it is a Go application, meaning the symbol names are still
there. If you want to look into reversing it yourself, I personally used Ghidra
as the tool. You want to look for
`github.com/zsa/keymapp/backend.(*Backend).OpenDevice`, as this is the
entry-point to most of the protocol. Keymapp is a Wail's app and this function
is called from the Javascript frontend. Go has reflection and this is how both
this function and all Keymapp socket API functions get called.
