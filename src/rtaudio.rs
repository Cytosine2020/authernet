#[link(name="crtaudio", kind="static")]
extern "C" {
    // this is rustified prototype of the function from our C library
    fn rtaudio_print_hosts();
}

pub fn print_hosts() {
    unsafe { rtaudio_print_hosts() }
}
