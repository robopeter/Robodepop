#![feature(iter_map_windows)]

use clap::Parser;
use flac::StreamReader;
use robo_depop_plugin::clean_data;
use std::{fs::File, io::Read, path::PathBuf};

/// Simple program to greet a person
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Input file
    #[arg(short, long)]
    input: PathBuf,

    #[arg(short, long)]
    output: PathBuf,
}

pub fn main() {
    let args = Args::parse();

    let mut buf = vec![];

    File::open(args.input)
        .expect("Could not open input file")
        .read_to_end(&mut buf)
        .expect("Could not read whole file!");

    match StreamReader::<File>::from_buffer(&buf) {
        Ok(mut stream) => {
            if stream.info().channels != 1 {
                eprintln!("Given FLAC file is more than one channel");
                return;
            }

            let spec = hound::WavSpec {
                channels: 1,
                sample_rate: stream.info().sample_rate,
                bits_per_sample: stream.info().bits_per_sample as u16,
                sample_format: hound::SampleFormat::Int,
            };

            let all_data: Vec<i32> = stream.iter::<i32>().collect();
            let cleaned = clean_data(&all_data);

            let mut writer = hound::WavWriter::create(args.output, spec).unwrap();
            for sample in cleaned {
                writer
                    .write_sample(sample)
                    .expect("Should be able to write sample!");
            }
        }
        Err(error) => println!("{:?}", error),
    }
}
