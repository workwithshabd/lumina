#![no_main]

use libfuzzer_sys::fuzz_target;
use atihsrah::processor::image_rs::ImageRsProcessor;
use atihsrah::processor::ImageProcessor;

fuzz_target!(|data: &[u8]| {
    let processor = ImageRsProcessor::new();

    // Must never panic on arbitrary byte inputs
    let _ = processor.probe(data);
});

