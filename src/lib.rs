#![feature(iter_map_windows)]
#![allow(dead_code, non_snake_case, non_upper_case_globals)]

/// The plugin portion of this code is based on the "Gain" example found here:
/// https://github.com/robbert-vdh/nih-plug/tree/master/plugins/examples/gain
/// which is ISC licensed Copyright (c) 2022-2024 Robbert van der Helm
use atomic_float::AtomicF32;
use core::f32;
use nih_plug::prelude::*;
use nih_plug_iced::IcedState;
use std::sync::Arc;

mod editor;

/// The time it takes for the peak meter to decay by 12 dB after switching to complete silence.
const PEAK_METER_DECAY_MS: f64 = 150.0;

/// This is mostly identical to the gain example, minus some fluff, and with a GUI.
pub struct Gain {
    params: Arc<GainParams>,

    /// Needed to normalize the peak meter's response based on the sample rate.
    peak_meter_decay_weight: f32,
    /// The current data for the peak meter. This is stored as an [`Arc`] so we can share it between
    /// the GUI and the audio processing parts. If you have more state to share, then it's a good
    /// idea to put all of that in a struct behind a single `Arc`.
    ///
    /// This is stored as voltage gain.
    peak_meter: Arc<AtomicF32>,

    working_buffer: Vec<f32>,
}

#[derive(Params)]
struct GainParams {
    /// The editor state, saved together with the parameter state so the custom scaling can be
    /// restored.
    #[persist = "editor-state"]
    editor_state: Arc<IcedState>,

    #[id = "gain"]
    pub gain: FloatParam,
}

impl Default for Gain {
    fn default() -> Self {
        Self {
            params: Arc::new(GainParams::default()),

            peak_meter_decay_weight: 1.0,
            peak_meter: Arc::new(AtomicF32::new(util::MINUS_INFINITY_DB)),
            working_buffer: Vec::new(),
        }
    }
}

impl Default for GainParams {
    fn default() -> Self {
        Self {
            editor_state: editor::default_state(),

            // See the main gain example for more details
            gain: FloatParam::new(
                "Gain",
                util::db_to_gain(0.0),
                FloatRange::Skewed {
                    min: util::db_to_gain(-30.0),
                    max: util::db_to_gain(30.0),
                    factor: FloatRange::gain_skew_factor(-30.0, 30.0),
                },
            )
            .with_smoother(SmoothingStyle::Logarithmic(50.0))
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_gain_to_db(2))
            .with_string_to_value(formatters::s2v_f32_gain_to_db()),
        }
    }
}

impl Plugin for Gain {
    const NAME: &'static str = "Gain GUI (iced)";
    const VENDOR: &'static str = "Robopeter";
    const URL: &'static str = "https://robopeter.com";
    const EMAIL: &'static str = "peter@robopeter.com";

    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(2),
            main_output_channels: NonZeroU32::new(2),
            ..AudioIOLayout::const_default()
        },
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(1),
            main_output_channels: NonZeroU32::new(1),
            ..AudioIOLayout::const_default()
        },
    ];

    const SAMPLE_ACCURATE_AUTOMATION: bool = true;

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        editor::create(
            self.params.clone(),
            self.peak_meter.clone(),
            self.params.editor_state.clone(),
        )
    }

    fn initialize(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        // After `PEAK_METER_DECAY_MS` milliseconds of pure silence, the peak meter's value should
        // have dropped by 12 dB
        self.peak_meter_decay_weight = 0.25f64
            .powf((buffer_config.sample_rate as f64 * PEAK_METER_DECAY_MS / 1000.0).recip())
            as f32;
        self.working_buffer = vec![0.0; buffer_config.max_buffer_size as usize + 10];
        true
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        for (_, block) in buffer.iter_blocks(128) {
            let block_channels = block.into_iter();

            for channel in block_channels {
                self.clean_data_f(channel);

                let mut amplitude: f32 = channel.iter().sum();
                let num_samples = channel.len();

                // To save resources, a plugin can (and probably should!) only perform expensive
                // calculations that are only displayed on the GUI while the GUI is open
                if self.params.editor_state.is_open() {
                    amplitude = (amplitude / num_samples as f32).abs();
                    let current_peak_meter =
                        self.peak_meter.load(std::sync::atomic::Ordering::Relaxed);
                    let new_peak_meter = if amplitude > current_peak_meter {
                        amplitude
                    } else {
                        current_peak_meter * self.peak_meter_decay_weight
                            + amplitude * (1.0 - self.peak_meter_decay_weight)
                    };

                    self.peak_meter
                        .store(new_peak_meter, std::sync::atomic::Ordering::Relaxed)
                }
            }
        }

        ProcessStatus::Normal
    }
}

impl ClapPlugin for Gain {
    const CLAP_ID: &'static str = "com.robopeter.robo_depop_plugin-iced";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("Remove single sample pops");
    const CLAP_MANUAL_URL: Option<&'static str> = Some(Self::URL);
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::AudioEffect,
        ClapFeature::Stereo,
        ClapFeature::Mono,
        ClapFeature::Utility,
    ];
}

impl Vst3Plugin for Gain {
    const VST3_CLASS_ID: [u8; 16] = *b"RoboDepopIcedAaA";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Tools];
}

nih_export_clap!(Gain);
nih_export_vst3!(Gain);

impl Gain {
    fn clean_data_f(&mut self, data: &mut [f32]) {
        clean_data_f_inner(data, &mut self.working_buffer);
    }
}

fn clean_data_f_inner(data: &mut [f32], working_buffer: &mut [f32]) {
    working_buffer[0] = f32::MAX;
    working_buffer[1] = f32::MIN;

    // We do this manually here to prevent a sneeky allocation which seems to
    // occur somewhere in the codepath of the suggested way to do this.
    #[allow(clippy::manual_memcpy)]
    for i in 0..data.len() {
        working_buffer[i + 2] = data[i];
    }

    working_buffer[data.len() + 2] = f32::MAX;
    working_buffer[data.len() + 3] = f32::MIN;

    for i in 0..data.len() {
        let a = working_buffer[i];
        let b = working_buffer[i + 1];
        let c = working_buffer[i + 2];
        let d = working_buffer[i + 3];
        let e = working_buffer[i + 4];
        let point = c;
        let min = (a).min(b).min(d).min(e);
        let max = (a).max(b).max(d).max(e);
        let distance = (max as f64 - min as f64).abs();
        let avg = (max as f64 + min as f64) / 2.0;

        data[i] =
            if point as f64 > (avg + distance * 2.0) || (point as f64) < (avg - distance * 2.0) {
                avg as f32
            } else {
                point
            }
    }
}

/// This was a previous attempt and no longer used... kept for archival purposes
fn clean_data_old(data: &[i32]) -> Vec<i32> {
    let mut out = Vec::with_capacity(data.len());

    out.append(&mut data.iter().take(5).copied().collect());

    for i in 6..(data.len() - 4) {
        let mut window: Vec<i32> = data.iter().skip(i - 3).take(5).copied().collect();
        let point = window.remove(2);

        let min = *window.iter().reduce(|acc, x| acc.min(x)).unwrap();
        let max = *window.iter().reduce(|acc, x| acc.max(x)).unwrap();

        let distance = (max - min).abs();
        let avg = (max + min) / 2;
        if point > (avg + distance * 2) || point < (avg - distance * 2) {
            out.push(avg);
        } else {
            out.push(point);
        }
    }
    out.append(&mut data.iter().skip(data.len() - 5).copied().collect());
    out
}

fn clean_data_f(data: &[f32]) -> Vec<f32> {
    println!("Length: {}", data.len() + 4);
    let mut data_copy = Vec::with_capacity(data.len() + 4);
    data_copy.push(f32::MAX);
    data_copy.push(f32::MIN);
    data_copy.extend(data.iter());
    data_copy.push(f32::MAX);
    data_copy.push(f32::MIN);

    let clean = data_copy
        .iter()
        .map_windows(|[a, b, c, d, e]| {
            let point = **c;
            let min = (*a).min(**b).min(**d).min(**e);
            let max = (*a).max(**b).max(**d).max(**e);
            let distance = (max as f64 - min as f64).abs();
            let avg = (max as f64 + min as f64) / 2.0;
            if point as f64 > (avg + distance * 2.0) || (point as f64) < (avg - distance * 2.0) {
                avg as f32
            } else {
                point
            }
        })
        .collect::<Vec<f32>>();

    clean
}

pub fn clean_data(data: &[i32]) -> Vec<i32> {
    let mut data_copy = Vec::with_capacity(data.len() + 4);
    data_copy.push(i32::MAX);
    data_copy.push(i32::MIN);
    data_copy.extend(data.iter());
    data_copy.push(i32::MAX);
    data_copy.push(i32::MIN);

    let clean = data_copy
        .iter()
        .map_windows(|[a, b, c, d, e]| {
            let point = **c;
            let min = *(*a).min(*b).min(*d).min(*e);
            let max = *(*a).max(*b).max(*d).max(*e);
            let distance = (max as i64 - min as i64).abs();
            let avg = (max as i64 + min as i64) / 2;
            if point as i64 > (avg + distance * 2) || (point as i64) < (avg - distance * 2) {
                avg.try_into().unwrap_or({
                    if avg > (i32::MAX as i64) {
                        i32::MAX
                    } else {
                        i32::MIN
                    }
                })
            } else {
                point
            }
        })
        .collect::<Vec<i32>>();

    clean
}

#[cfg(test)]
mod tests {
    use super::*;

    fn print_value(sample: i32, weird: u8, count: i32) {
        // Iterate over each decoded sample
        let width: i32 = 1000 * sample / 0b0111_1111_1111_1111_1111_1111;
        let before = String::from_utf8(vec![weird; (100 + width).try_into().unwrap()]).unwrap();
        let after = String::from_utf8(vec![weird; (100 - width).try_into().unwrap()]).unwrap();
        println!("{}*{}{} ({}) C: {}", before, after, sample, width, count)
    }

    #[test]
    fn it_works() {
        use flac::StreamReader;
        use std::fs::File;

        // match StreamReader::<File>::from_file("C:\\Users\\Peter\\Videos\\TEST.flac") {
        match StreamReader::<File>::from_file("docs\\trim.flac") {
            Ok(mut stream) => {
                // Copy of `StreamInfo` to help convert to a different audio format.
                let info = stream.info();

                println!("{}", info.bits_per_sample);

                let mut count = 0;

                let mut prev = 0;
                // let mut last_weird = false;

                // The explicit size for `Stream::iter` is the resulting decoded
                // sample. You can usually find out the desired size of the
                // samples with `info.bits_per_sample`.
                for sample in stream.iter::<i32>().skip(65) {
                    //.skip(80640000) {
                    count += 1;
                    if count > 48000 {
                        break;
                    }
                    let mut weird = b' ';
                    if (sample > (prev + 80000)) || (sample < (prev - 80000)) {
                        // println!("count: {}", count);
                        weird = b'-';
                    }
                    // if weird == b'-' {
                    //     print_value(prev, weird, count - 1);
                    //     print_value(sample, weird, count);
                    //     last_weird = true;
                    // }
                    // if last_weird {
                    //     print_value(sample, weird, count);
                    //     last_weird = false;
                    // }

                    prev = sample;
                    if count > 33400 && count < 33600 {
                        print_value(sample, weird, count);
                    }
                }
            }
            Err(error) => println!("{:?}", error),
        }
    }

    use plotters::prelude::*;

    fn plot_data(filename: &str, data: &[i32]) {
        let max = *data.iter().reduce(|acc, x| acc.max(x)).unwrap() as f32;
        let min = *data.iter().reduce(|acc, x| acc.min(x)).unwrap() as f32;

        let root = BitMapBackend::new(&filename, (1600, 1600)).into_drawing_area();
        root.fill(&WHITE).unwrap();
        let mut chart = ChartBuilder::on(&root)
            .caption("data", ("sans-serif", 50).into_font())
            .margin(5)
            .x_label_area_size(30)
            .y_label_area_size(30)
            .build_cartesian_2d((0 as f32)..(data.len() as f32), min..max)
            .unwrap();

        chart.configure_mesh().draw().unwrap();

        chart
            .draw_series(
                LineSeries::new(
                    data.iter()
                        .enumerate()
                        .map(|(x, y)| (x as f32, *y as f32))
                        .collect::<Vec<(f32, f32)>>(),
                    &RED,
                )
                .point_size(2),
            )
            .unwrap()
            .label("data")
            .legend(|(x, y)| PathElement::new(vec![(x, y), (x + 20, y)], RED));

        root.present().unwrap();
    }

    /// This test case generates the pictures used in the documentation
    #[test]
    fn plotters() {
        use flac::StreamReader;
        use std::fs::File;

        let window_size = 5;
        let iterations = 10;

        // match StreamReader::<File>::from_file("C:\\Users\\Peter\\Videos\\TEST.flac") {
        match StreamReader::<File>::from_file("docs\\trim.flac") {
            Ok(mut stream) => {
                let all_data: Vec<i32> = stream
                    .iter::<i32>()
                    .skip(33400 + 65)
                    .take(iterations + window_size + 200)
                    .collect();

                plot_data("corrupted.png", &all_data);

                let cleaned_data = clean_data(&all_data);
                plot_data("fixed.png", &cleaned_data);

                // println!("all: {:?}", all_data);
                // println!("cle: {:?}", cleaned_data);

                // for i in 0..iterations {
                //     let data: Vec<i32> =
                //         all_data.iter().skip(i).take(window_size).copied().collect();

                //     let filename = format!("{}.png", i);
                //     plot_data(&filename, &data);
                // }
            }
            Err(error) => println!("{:?}", error),
        }
    }
}
