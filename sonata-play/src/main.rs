// Sonata
// Copyright (c) 2019 The Sonata Project Developers.
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::default::Default;
use std::fs::File;
use std::path::Path;
use clap::{Arg, App};
use sonata;
use sonata::core::errors::{Result, unsupported_error};
use sonata::core::audio::*;
use sonata::core::codecs::DecoderOptions;
use sonata::core::formats::{Cue, FormatReader, Hint, FormatOptions, ProbeDepth, ProbeResult, ColorMode, Visual, Stream};
use sonata::core::tags::Tag;

#[cfg(target_os = "linux")]
use libpulse_binding as pulse;
#[cfg(target_os = "linux")]
use libpulse_simple_binding as psimple;

fn main() {
    let matches = App::new("Sonata Play")
                        .version("1.0")
                        .author("Philip Deljanov <philip.deljanov@gmail.com>")
                        .about("Play audio files with Sonata")
                        .arg(Arg::with_name("seek")
                            .long("seek")
                            .short("-s")
                            .value_name("TIMESTAMP")
                            .help("Seek to the given timestamp")
                            .conflicts_with_all(&[ "verify", "decode-only", "verify-only", "probe-only" ]))
                        .arg(Arg::with_name("decode-only")
                            .long("decode-only")
                            .help("Decodes, but does not play the audio")
                            .conflicts_with_all(&[ "probe-only", "verify-only", "verify" ]))
                        .arg(Arg::with_name("probe-only")
                            .long("probe-only")
                            .help("Only probe the file for metadata")
                            .conflicts_with_all(&[ "decode-only", "verify-only" ]))
                        .arg(Arg::with_name("verify-only")
                            .long("verify-only")
                            .help("Verifies the decoded audio is valid, but does not play the audio")
                            .conflicts_with_all(&[ "verify" ]))
                        .arg(Arg::with_name("verify")
                            .long("verify")
                            .short("-V")
                            .help("Verifies the decoded audio is valid during playback"))
                       .arg(Arg::with_name("verbose")
                            .short("v")
                            .multiple(true)
                            .help("Sets the level of verbosity"))
                        .arg(Arg::with_name("FILE")
                            .help("Sets the input file to use")
                            .required(true)
                            .index(1))
                        .get_matches();

    // Get the file path option.
    let path = Path::new(matches.value_of("FILE").unwrap());

    // Create a hint to help the format registry guess what format reader is appropriate for file at the given file 
    // path.
    let mut hint = Hint::new();

    // Use the file extension as a hint.
    if let Some(extension) = path.extension() {
        hint.with_extension(extension.to_str().unwrap());
    }

    // Open the given file.
    // TODO: Catch errors.
    let file = Box::new(File::open(path).unwrap());

    // Use the format registry to pick a format reader for the given file and instantiate it with a default set of 
    // options.
    let format_options = FormatOptions { ..Default::default() };
    let mut reader = sonata::default::get_formats().guess(&hint, file, &format_options).unwrap();

    // Probe the file using the format reader to verify the file is actually supported.
    let probe_info = reader.probe(ProbeDepth::Deep).unwrap();

    match probe_info {
        // The file was not actually supported by the format reader.
        ProbeResult::Unsupported => {
            eprintln!("File not supported!");
        },
        // The file is supported by the format reader.
        ProbeResult::Supported => {
            // Verify only mode decodes and always verifies the audio, but doese not play it.
            if matches.is_present("verify-only") {
                let options = DecoderOptions { verify: true, ..Default::default() };
                decode_only(reader, &options).unwrap_or_else(|err| { eprintln!("Err: {}", err) });
            }
            // Decode only mode decodes the audio, but not does verify it.
            else if matches.is_present("decode-only") {
                let options = DecoderOptions { verify: false, ..Default::default() };
                decode_only(reader, &options).unwrap_or_else(|err| { eprintln!("Err: {}", err) });
            }
            // Probe only mode prints information about the format, streams, metadata, etc.
            else if matches.is_present("probe-only") {
                pretty_print_format(&path, &reader);
            }
            // If nothing else, decode and play the audio.
            else {
                pretty_print_format(&path, &reader);

                // Seek to the desired timestamp if requested.
                match matches.value_of("seek") {
                    Some(seek_value) => {
                        let pos = seek_value.parse::<f64>().unwrap();
                        reader.seek(Timestamp::Time(pos)).unwrap();
                    },
                    None => (),
                };

                // Set the decoder options.
                let options = DecoderOptions { 
                    verify: matches.is_present("verify"), 
                    ..Default::default()
                };

                // Commence playback.
                play(reader, &options).unwrap_or_else(|err| { eprintln!("Err: {}", err) });
            }
        }
    }
}

fn decode_only(mut reader: Box<dyn FormatReader>, decode_options: &DecoderOptions) -> Result<()> {
    // Get the default stream.
    // TODO: Allow stream selection.
    let stream = reader.default_stream().unwrap();

    // Create a decoder for the stream.
    let mut decoder = sonata::default::get_codecs().make(&stream.codec_params, &decode_options)?;

    // Decode all packets.
    loop {
        match decoder.decode(reader.next_packet()?) {
            Err(err) => {
                decoder.close();
                return Err(err);
            },
            Ok(_) => ()
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn play(_: Box<dyn FormatReader>, _: &DecoderOptions) -> Result<()> {
    // TODO: Support the platform.
    unsupported_error("Playback is not supported on your platform.")
}

#[cfg(target_os = "linux")]
fn play(mut reader: Box<dyn FormatReader>, decode_options: &DecoderOptions) -> Result<()> {
    // Get the default stream.
    // TODO: Allow stream selection.
    let stream = reader.default_stream().unwrap();

    // Create a decoder for the stream.
    let mut decoder = sonata::default::get_codecs().make(&stream.codec_params, &decode_options)?;

    // Decode the first packet and create the PulseAudio device using the signal specification of 
    // the buffer.
    let (pa, mut samples) = match decoder.decode(reader.next_packet()?) {
        Err(err) => {
            decoder.close();
            return Err(err);
        },
        Ok(decoded) => {
            // Get the buffer spec.
            let spec = decoded.spec();

            // Get the buffer duration.
            let duration = Duration::Frames(decoded.capacity() as u64);

            // An interleaved buffer is required to send data to PulseAudio. Sse a SampleBuffer to
            // move data between Sonata AudioBuffers and the byte buffers required by PulseAudio.
            let mut samples = SampleBuffer::<i32>::new(duration, &spec);

            // Create a PulseAudio stream specification.
            let pa_spec = pulse::sample::Spec {
                format: pulse::sample::SAMPLE_S32NE,
                channels: spec.channels.len() as u8,
                rate: spec.rate,
            };

            assert!(pa_spec.is_valid());

            // Create a PulseAudio connection.
            let pa = psimple::Simple::new(
                None,                                   // Use default server
                "Sonata Player",                        // Application name
                pulse::stream::Direction::Playback,     // Playback stream
                None,                                   // Default playback device
                "Music",                                // Description of the stream
                &pa_spec,                               // Signal specificaiton
                None,                                   // Default channel map
                None                                    // Default buffering attributes
            ).unwrap();

            // Interleave samples for PulseAudio into the sample buffer.
            samples.copy_interleaved_ref(decoded, Dither::None);

            // Write interleaved samples to PulseAudio.
            pa.write(samples.as_bytes()).unwrap();

            (pa, samples)
        }
    };

    // Decode the remaining frames.
    loop {
        match decoder.decode(reader.next_packet()?) {
            Err(err) => {
                decoder.close();
                return Err(err);
            },
            Ok(decoded) => {
                samples.copy_interleaved_ref(decoded, Dither::None);
                pa.write(samples.as_bytes()).unwrap();
            }
        }
    }

}

fn pretty_print_format(path: &Path, reader: &Box<dyn FormatReader>) {
    println!("+ {}", path.display());
    pretty_print_streams(reader.streams());
    pretty_print_tags(reader.tags());
    pretty_print_cues(reader.cues());
    pretty_print_visuals(reader.visuals());
    println!("-");
}

fn pretty_print_streams(streams: &[Stream]) {
    if streams.len() > 0 {
        println!("|");
        println!("| // Streams //");

        for (idx, stream) in streams.iter().enumerate() {
            let params = &stream.codec_params;

            println!("|     [{:0>2}] Codec:           {}", idx + 1, params.codec);
            if let Some(sample_rate) = params.sample_rate {
                println!("|          Sample Rate:     {}", sample_rate);
            }
            if let Some(n_frames) = params.n_frames {
                println!("|          Frames:          {}", n_frames);
            }
            if let Some(sample_format) = params.sample_format {
                println!("|          Sample Format:   {:?}", sample_format);
            }
            if let Some(bits_per_sample) = params.bits_per_sample {
                println!("|          Bits per Sample: {}", bits_per_sample);
            }
            if let Some(channels) = params.channels {
                println!("|          Channel(s):      {}", channels.len());
                println!("|          Channel Map:     {}", channels);
            }
            if let Some(channel_layout) = params.channel_layout {
                println!("|          Channel Layout:  {:?}", channel_layout);
            }
            if let Some(language) = &stream.language {
                println!("|          Language:        {}", language);
            }

        }
    }
}

fn pretty_print_cues(cues: &[Cue]) {
    if cues.len() > 0 {
        println!("|");
        println!("| // Cues //");

        for (idx, cue) in cues.iter().enumerate() {
            println!("|     [{:0>2}] Track:      {}", idx + 1, cue.index);
            println!("|          Timestamp:  {}", cue.start_ts);

            // Print tags associated with the Cue.
            if cue.tags.len() > 0 {
                println!("|          Tags:");

                for (tidx, tag) in cue.tags.iter().enumerate() {
                    if let Some(std_key) = tag.std_key {
                        println!("{}", pretty_print_tag_item(tidx + 1, &format!("{:?}", std_key), &tag.value, 21));
                    }
                    else {
                        println!("{}", pretty_print_tag_item(tidx + 1, &tag.key, &tag.value, 21));
                    }
                }
            }

            // Print any sub-cues.
            if cue.points.len() > 0 {
                println!("|          Sub-Cues:");

                for (ptidx, pt) in cue.points.iter().enumerate() {
                    println!("|                      [{:0>2}] Offset:    {:?}", ptidx + 1, pt.start_offset_ts);

                    // Start the number of sub-cue tags, but don't print them.
                    if pt.tags.len() > 0 {
                        println!("|                           Sub-Tags:  {} (not listed)", pt.tags.len());
                    }
                }
            }

        }
    }
}

fn pretty_print_tags(tags: &[Tag]) {
    if tags.len() > 0 {
        println!("|");
        println!("| // Tags //");
        
        let mut idx = 1;

        // Print tags with a standard tag key first, these are the most common tags.
        for tag in tags.iter().filter(| tag | tag.is_known()) {
            if let Some(std_key) = tag.std_key {
                println!("{}", pretty_print_tag_item(idx, &format!("{:?}", std_key), &tag.value, 4));
            }
            idx += 1;
        }

        // Print the remaining tags with keys truncated to 26 characters.
        for tag in tags.iter().filter(| tag | !tag.is_known()) {
            println!("{}", pretty_print_tag_item(idx, &tag.key, &tag.value, 4));
            idx += 1;
        }
    }
}

fn pretty_print_visuals(visuals: &[Visual]) {
    if visuals.len() > 0 {
        println!("|");
        println!("| // Visuals //");

        for (idx, visual) in visuals.iter().enumerate() {

            if let Some(usage) = visual.usage {
                println!("|     [{:0>2}] Usage:      {:?}", idx + 1, usage);
                println!("|          Media Type: {}", visual.media_type);
            }
            else {
                println!("|     [{:0>2}] Media Type: {}", idx + 1, visual.media_type);
            }
            if let Some(dimensions) = visual.dimensions {
                println!("|          Dimensions: {} px x {} px", dimensions.width, dimensions.height);
            }
            if let Some(bpp) = visual.bits_per_pixel {
                println!("|          Bits/Pixel: {}", bpp);
            }
            if let Some(ColorMode::Indexed(colors)) = visual.color_mode {
                println!("|          Palette:    {} colors", colors);
            }
            println!("|          Size:       {} bytes", visual.data.len());

            // Print out tags similar to how regular tags are printed.
            if visual.tags.len() > 0 {
                println!("|          Tags:");
            }

            for (tidx, tag) in visual.tags.iter().enumerate() {
                if let Some(std_key) = tag.std_key {
                    println!("{}", pretty_print_tag_item(tidx + 1, &format!("{:?}", std_key), &tag.value, 21));
                }
                else {
                    println!("{}", pretty_print_tag_item(tidx + 1, &tag.key, &tag.value, 21));
                }
            }
        }
    }
}

fn pretty_print_tag_item(idx: usize, key: &str, value: &str, indent: usize) -> String {
    let key_str = match key.len() {
        0...28 => format!("| {:w$}[{:0>2}] {:<28} : ", "", idx, key, w = indent),
        _ => format!("| {:w$}[{:0>2}] {:.<28} : ", "", idx, key.split_at(26).0, w = indent),
    };

    let line_prefix = format!("\n| {:w$} : ", "", w = indent + 4 + 28 + 1);
    let line_wrap_prefix = format!("\n| {:w$}   ", "", w = indent + 4 + 28 + 1);

    let mut out = String::new();

    out.push_str(&key_str);

    for (wrapped, line) in value.lines().enumerate() {
        if wrapped > 0 {
            out.push_str(&line_prefix);
        }

        let mut chars = line.chars();
        let split = (0..)
            .map(|_| chars.by_ref().take(72).collect::<String>())
            .take_while(|s| !s.is_empty())
            .collect::<Vec<_>>();

        out.push_str(&split.join(&line_wrap_prefix));
    }

    out
}