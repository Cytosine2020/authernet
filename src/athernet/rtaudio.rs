use std::{ffi::c_void, ops::Deref};


#[derive(std::fmt::Debug)]
pub enum StreamError {
    UnknownError,
}

impl std::fmt::Display for StreamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RTAudio stream error!")
    }
}

impl std::error::Error for StreamError {}

pub struct Stream {
    inner: *mut c_void,
    _closure: Box<dyn FnMut(&mut [i16])>,
}

impl Stream {
    pub unsafe fn new(inner: *mut c_void, _closure: Box<dyn FnMut(&mut [i16])>) -> Self {
        Self { inner, _closure }
    }
}

impl Drop for Stream {
    fn drop(&mut self) { unsafe { rtaudio_destroy_stream(self.inner); } }
}

extern "C" {
    fn rtaudio_print_hosts();

    fn rtaudio_create_output_stream(callback: *const c_void, data: *mut c_void) -> *mut c_void;

    fn rtaudio_create_input_stream(callback: *const c_void, data: *mut c_void) -> *mut c_void;

    fn rtaudio_destroy_stream(stream: *mut c_void);
}

#[allow(dead_code)]
pub fn print_hosts() { unsafe { rtaudio_print_hosts() } }

#[inline(always)]
unsafe extern "C" fn callback_adapter<F>(data: *mut c_void, buffer: *mut i16, size: usize)
    where F: FnMut(&mut [i16]) + Send + 'static
{
    (*(data as *mut F))(&mut *std::ptr::slice_from_raw_parts_mut(buffer, size));
}

unsafe fn result_unwrap(result: *mut c_void) -> Result<*mut c_void, StreamError> {
    if result == 0 as _ {
        Err(StreamError::UnknownError)
    } else {
        Ok(result)
    }
}

pub fn create_output_stream<F>(callback_: F) -> Result<Stream, StreamError>
    where F: FnMut(&mut [i16]) + Send + 'static
{
    let callback = Box::new(callback_);

    unsafe {
        Ok(Stream::new(result_unwrap(rtaudio_create_output_stream(
            callback_adapter::<F> as *const () as _,
            callback.deref() as *const _ as _,
        ))?, callback))
    }
}

pub fn create_input_stream<F>(callback_: F) -> Result<Stream, StreamError>
    where F: FnMut(&mut [i16]) + Send + 'static
{
    let callback = Box::new(callback_);

    unsafe {
        Ok(Stream::new(result_unwrap(rtaudio_create_input_stream(
            callback_adapter::<F> as *const () as _,
            callback.deref() as *const _ as _,
        ))?, callback))
    }
}
